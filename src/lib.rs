#![no_main]
#![no_std]
#![feature(alloc_error_handler)]

use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};
use serde::Serialize;
use coap_lite::{CoapRequest, ContentFormat, RequestType};
use coap_lite::error::MessageError;
use alloc_cortex_m::CortexMHeap;
use defmt::info;
use embassy_nrf as _;
use embassy_time::TimeoutError;
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
    Timeout(TimeoutError),
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

impl From<nrf_modem::error::Error> for Error {
fn from(e: nrf_modem::error::Error) -> Self {
    Self::NrfModem(e)
    }
}

impl From<TimeoutError> for Error {
    fn from(e: TimeoutError) -> Self {
        Self::Timeout(e)
    }
}

/// Structure to hold our payload buffer (heapless Vec)
#[derive(Serialize)]
pub struct TankLevel {
    pub data: Vec<i16,3>,
}

impl TankLevel {
    pub fn new() -> Self {
        TankLevel { data: Vec::new() }
    }
}

/// Struct for our server socket connection
pub struct Dtls {
    socket: DtlsSocket,
}

impl Dtls {
    /// Constructor for a DTLS encrypted socket
    pub async fn new() -> Result<Self, Error> {
        let socket = DtlsSocket::new(
            PeerVerification::Enabled,
            &[config::SECURITY_TAG]
        ).await?;

        Ok(Self { socket })
    }

    /// Create CoAP request, serialize payload, and transimt data
    /// request path can start with .s/ for LightDB Stream or .d/ LightDB State for Golioth IoT
    pub async fn transmit_payload(&mut self, tank_level: &TankLevel) -> Result<(), Error> {
        let mut request: CoapRequest<DtlsSocket> = CoapRequest::new();
        // request.message.header.message_id = MESSAGE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        request.set_method(RequestType::Post);
        request.set_path(".s/tank_level");
        request.message.set_content_format(ContentFormat::ApplicationJSON);
        let json = serde_json::to_vec(tank_level)?;
        info!("{}",defmt::Debug2Format(&json));
        request.message.payload = json;

        self.socket.connect(
            SERVER_URL,
            SERVER_PORT
        ).await?;

        self.socket.send(&request.message.to_bytes()?).await?;

        Ok (())
    }
}

/// Terminates the application and makes `probe-run` exit with exit-code = 0
pub fn exit() -> ! {
    loop {
        cortex_m::asm::bkpt();
    }
}

/// An allocator is required for the coap-lite lib
#[global_allocator]
static ALLOCATOR: CortexMHeap = CortexMHeap::empty();

static mut HEAP_DATA: [MaybeUninit<u8>; 16384] = [MaybeUninit::uninit(); 16384];

pub fn alloc_init() {
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

/// Default alloc error handler for when allocation fails
#[alloc_error_handler]
fn alloc_error(_: core::alloc::Layout) -> ! {
    cortex_m::asm::udf()
}
