#![no_main]
#![no_std]

use coap_lite::{CoapRequest, ContentFormat, RequestType};
use coap_lite::error::MessageError;
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

// Structure to hold our payload buffer (heapless Vec)
#[derive(Serialize)]
pub struct TankLevel {
    pub data: Vec<i16,3>,
}

impl TankLevel {
    pub fn new() -> Self {
        Payload { data: Vec::new() }
    }
}

// Struct for our server connection
pub struct Dtls {
    socket: DtlsSocket,
}

impl Dtls {
    pub fn new() -> Self {
        let socket = DtlsSocket::new(
            PeerVerification::Enabled,
            &[config::SECURITY_TAG]
        );

        Self { socket }
    }

    pub fn transmit_payload(&mut self, tank_level: &TankLevel) -> Result<(), Error> {
        let mut request = CoapRequest::new();
        // request.message.header.message_id = MESSAGE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        request.set_method(RequestType::Post);
        request.set_path("data");
        request.message.set_content_format(ContentFormat::ApplicationJSON);
        request.message.payload = serde_json::to_vec(&tank_level.data)?;

        self.socket.connect(
            SERVER_URL,
            SERVER_PORT
        )?.await;

        self.socket.send(&request.message.to_bytes()?)?.await;

        self.socket.deactivate();

        Ok (())
    }
}

/// Terminates the application and makes `probe-run` exit with exit-code = 0
pub fn exit() -> ! {
    loop {
        cortex_m::asm::bkpt();
    }
}