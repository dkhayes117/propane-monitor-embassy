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
use at_commands::parser::CommandParser;
use defmt::{info, Debug2Format, Format};
use embassy_nrf as _;
use embassy_time::{Duration, TimeoutError, Timer};
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
#[derive(Debug, Serialize)]
pub struct Payload {
    pub battery: u32,
    pub data: Vec<TankLevel, 6>,
    pub signal: i32,
    pub timeouts: u8,
}

impl Payload {
    pub fn new() -> Self {
        Payload {
            battery: 0,
            data: Vec::new(),
            signal: 0,
            timeouts: 0,
        }
    }
}

/// Structure to hold our individual measure data
#[derive(Debug, Serialize)]
pub struct TankLevel {
    pub value: u8,
    pub timestamp: u32,
}

impl TankLevel {
    pub fn new(value: u8, timestamp: u32) -> Self {
        TankLevel { value, timestamp }
    }
}

pub async fn config_gnss() -> Result<(), Error> {
    // confgiure MAGPIO pins for GNSS
    info!("Configuring XMAGPIO pins for 1574-1577 MHz");
    nrf_modem::at::send_at::<0>("AT%XMAGPIO=1,0,0,1,1,1574,1577").await?;
    nrf_modem::at::send_at::<0>("AT%XCOEXO=1,1,1574,1577").await?;
    Ok(())
}

/// Function to retrieve GPS data from a single GNSS fix
pub async fn get_gnss_data() -> Result<(), Error> {
    let mut gnss = nrf_modem::gnss::Gnss::new().await?;
    let config = nrf_modem::gnss::GnssConfig::default();
    let mut iter = gnss.start_single_fix(config)?;

    if let Some(x) = futures::StreamExt::next(&mut iter).await {
        info!("{:?}", defmt::Debug2Format(&x.unwrap()));
    }
    Ok(())
}

/// Create CoAP request, serialize payload, and transimt data
/// request path can start with .s/ for LightDB Stream or .d/ LightDB State for Golioth IoT
pub async fn transmit_payload(payload: &mut Payload) -> Result<(), Error> {
    // Establish an LTE link
    let link = LteLink::new().await?;
    link.wait_for_link().await?;

    let sig_strength = get_signal_strength().await?;
    payload.signal = sig_strength;
    info!("Signal Strength: {} dBm", &sig_strength);


    let mut request: CoapRequest<DtlsSocket> = CoapRequest::new();
    // request.message.header.message_id = MESSAGE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    request.set_method(RequestType::Post);
    request.set_path(".s/tank_level");
    request
        .message
        .set_content_format(ContentFormat::ApplicationJSON);
    let json = serde_json::to_vec(payload)?;
    // info!("Payload: {:?}", Debug2Format(&payload));
    // info!("JSON Byte Vec: {:?}", Debug2Format(&json));
    request.message.payload = json;

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

/// Convert sensor ADC value into tank level percentage
pub fn convert_to_tank_level(x: i16) -> u8 {
    let val = ((0.0529 * x as f32) - 38.6794) as u8;
    if val > 100 {
        100
    } else if val < 10 {
        10
    } else {
        val
    }
}

/// Parse AT+CESQ command response and return a signal strength in dBm
/// Signal strength = -140 dBm + last int_parameter
async fn get_signal_strength() -> Result<i32, Error> {
    let command = nrf_modem::at::send_at::<32>("AT+CESQ").await?;

    let _cereg = CommandParser::parse(command.as_bytes())
        .expect_identifier(b"+CESQ:")
        .expect_int_parameter()
        .expect_int_parameter()
        .expect_int_parameter()
        .expect_int_parameter()
        .expect_int_parameter()
        .expect_int_parameter()
        .expect_identifier(b"\r\nOK\r\n")
        .finish()
        .map(|(_, _, _, _, _, signal)| signal);

    -140 + signal
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
