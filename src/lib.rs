#![no_main]
#![no_std]

use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};
use serde::Serialize;
use coap_lite::{CoapRequest, ContentFormat, RequestType};
use coap_lite::error::MessageError;
use alloc_cortex_m::CortexMHeap;
use embassy_nrf as _;
use heapless::Vec;
use nrf_modem::dtls_socket::{DtlsSocket, PeerVerification};
use {defmt_rtt as _, panic_probe as _};
use crate::config::{SERVER_PORT, SERVER_URL};

extern crate tinyrlibc;

pub mod config;

#[derive(Debug)]
pub enum Error {
    Coap(MessageError),
    Json(serde_json::error::Error),
    NrfModem(nrf_modem::error::Error),
}

impl From<MessageError> for Error {
    fn from(e: MessageError) -> Self {
        Self::Coap(e)
    }
}

impl From<serde_json::error::Error> for Error {
    fn from(e: serde_json::error::Error) -> Self {
        Self::Json(e)
    }
}

// impl From<nrf_modem::error::Error> for Error {
// fn from(e: From<nrf_modem::error::Error>) -> Self {
//     Self::NrfModem(e)
//     }
// }

// Structure to hold our payload buffer (heapless Vec)
#[derive(Serialize)]
pub struct TankLevel {
    pub data: Vec<i16,3>,
}

impl TankLevel {
    pub fn new() -> Self {
        TankLevel { data: Vec::new() }
    }
}

// Struct for our server connection
pub struct Dtls {
    socket: DtlsSocket,
}

impl Dtls {
    pub async fn new() -> Result<Self, Error> {
        let socket = DtlsSocket::new(
            PeerVerification::Enabled,
            &[config::SECURITY_TAG]
        ).await.unwrap();

        Ok( Self { socket } )
    }

    pub async fn transmit_payload<Endpoint>(&mut self, tank_level: &TankLevel) {
        let mut request:CoapRequest<Endpoint> = CoapRequest::new();
        // request.message.header.message_id = MESSAGE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        request.set_method(RequestType::Post);
        request.set_path("data");
        request.message.set_content_format(ContentFormat::ApplicationJSON);
        request.message.payload = serde_json::to_vec(&tank_level.data).unwrap();

        self.socket.connect(
            SERVER_URL,
            SERVER_PORT
        ).await.unwrap();

        self.socket.send(&request.message.to_bytes().unwrap()).await.unwrap();

        // self.socket.deactivate();

        // Ok (())
    }
}

/// Terminates the application and makes `probe-run` exit with exit-code = 0
pub fn exit() -> ! {
    loop {
        cortex_m::asm::bkpt();
    }
}

// An allocator is required for the coap-lite lib
#[global_allocator]
static ALLOCATOR: CortexMHeap = CortexMHeap::empty();

static mut HEAP_DATA: [MaybeUninit<u8>; 8192] = [MaybeUninit::uninit(); 8192];

pub fn init() {
    static ONCE: AtomicBool = AtomicBool::new(false);

    if ONCE
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        unsafe {
            ALLOCATOR.init(HEAP_DATA.as_ptr() as usize, HEAP_DATA.len());
        }
    }
}