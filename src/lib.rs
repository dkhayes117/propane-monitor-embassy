#![no_main]
#![no_std]
#![feature(alloc_error_handler)]

use crate::config::{PSK, PSK_ID, SECURITY_TAG, SERVER_PORT, SERVER_URL};
use alloc_cortex_m::CortexMHeap;
use coap_lite::error::MessageError;
use coap_lite::{CoapRequest, ContentFormat, RequestType};
use core::fmt::write;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};
use defmt::{info, Format};
use embassy_nrf as _;
use embassy_time::TimeoutError;
use heapless::{String, Vec};
use nrf_modem::dtls_socket::{DtlsSocket, PeerVerification};
use nrf_modem::lte_link::LteLink;
use serde::Serialize;
use {defmt_rtt as _, panic_probe as _};

extern crate alloc;
extern crate tinyrlibc;

pub mod config;

/// Credential Storage Management Types
#[derive(Clone, Copy, Format)]
#[allow(dead_code)]
enum CSMType {
    RootCert = 0,
    ClientCert = 1,
    ClientPrivateKey = 2,
    Psk = 3,
    PskId = 4,
    // ...
}

/// Crate error types
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

/// Payload to send over CoAP (Heapless Vec of Tanklevel Structs)
#[derive(Serialize)]
pub struct Payload {
    pub data: Vec<TankLevel, 3>,
}

impl Payload {
    pub fn new() -> Self {
        Payload { data: Vec::new() }
    }
}

/// Structure to hold our individual measure data
#[derive(Debug, Serialize)]
pub struct TankLevel {
    pub value: i16,
    pub timestamp: u32,
}

impl TankLevel {
    pub fn new(value: i16, timestamp: u32) -> Self {
        TankLevel { value, timestamp }
    }
}

/// Function to retrieve GPS data from a single GNSS fix
pub async fn gnss_data() -> Result<(), Error> {
    nrf_modem::configure_gnss_on_pca10090ns().await?;
    let mut gnss = nrf_modem::gnss::Gnss::new().await?;
    let config = nrf_modem::gnss::GnssConfig::default();
    let mut iter = gnss.start_single_fix(config)?;
    if let Some(x) = futures::StreamExt::next(&mut iter).await {
        defmt::println!("{:?}", defmt::Debug2Format(&x));
    }
    Ok(())
}

/// Create CoAP request, serialize payload, and transimt data
/// request path can start with .s/ for LightDB Stream or .d/ LightDB State for Golioth IoT
pub async fn transmit_payload(payload: &Payload) -> Result<(), Error> {
    let mut request: CoapRequest<DtlsSocket> = CoapRequest::new();
    // request.message.header.message_id = MESSAGE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    request.set_method(RequestType::Post);
    request.set_path(".s/tank_level");
    request
        .message
        .set_content_format(ContentFormat::ApplicationJSON);
    let json = serde_json::to_vec(payload)?;
    request.message.payload = json;

    // Establish an LTE link
    let link = LteLink::new().await?;
    link.wait_for_link().await?;

    // Create our DTLS socket
    let mut socket = DtlsSocket::new(PeerVerification::Enabled, &[SECURITY_TAG]).await?;

    socket.connect(SERVER_URL, SERVER_PORT).await?;

    socket.send(&request.message.to_bytes()?).await?;

    // The sockets would be dropped after the function call ends, but this explicit call allows them
    // to be dropped asynchronously
    socket.deactivate().await?;
    link.deactivate().await?;

    Ok(())
}
// }

/// This function deletes a key or certificate from the nrf modem
async fn key_delete(ty: CSMType) -> Result<(), Error> {
    let mut cmd: String<32> = String::new();
    write(
        &mut cmd,
        format_args!("AT%CMNG=3,{},{}", SECURITY_TAG, ty as u32),
    )
    .unwrap();
    nrf_modem::at::send_at::<32>(cmd.as_str()).await?;
    Ok(())
}

/// This function writes a key or certificate to the nrf modem
async fn key_write(ty: CSMType, data: &str) -> Result<(), Error> {
    let mut cmd: String<128> = String::new();
    write(
        &mut cmd,
        format_args!(r#"AT%CMNG=0,{},{},"{}""#, SECURITY_TAG, ty as u32, data),
    )
    .unwrap();

    nrf_modem::at::send_at::<128>(&cmd.as_str()).await?;

    Ok(())
}

/// Delete existing keys/certificates and loads new ones based on config.rs entries
pub async fn install_psk_id_and_psk() -> Result<(), Error> {
    assert!(
        !&PSK_ID.is_empty() && !&PSK.is_empty(),
        "PSK ID and PSK must not be empty. Set them in the `config` module."
    );

    key_delete(CSMType::PskId).await?;
    key_delete(CSMType::Psk).await?;

    key_write(CSMType::PskId, &PSK_ID).await?;
    key_write(CSMType::Psk, &encode_psk_as_hex(&PSK)).await?;

    Ok(())
}

fn encode_psk_as_hex(psk: &[u8]) -> String<128> {
    fn hex_from_digit(num: u8) -> char {
        if num < 10 {
            (b'0' + num) as char
        } else {
            (b'a' + num - 10) as char
        }
    }

    let mut s: String<128> = String::new();
    for ch in psk {
        s.push(hex_from_digit(ch / 16)).unwrap();
        s.push(hex_from_digit(ch % 16)).unwrap();
    }

    s
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

static mut HEAP_DATA: [MaybeUninit<u8>; 8196] = [MaybeUninit::uninit(); 8196];

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
