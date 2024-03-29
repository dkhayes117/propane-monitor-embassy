#![no_main]
#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;
extern crate tinyrlibc;

mod at;
mod config;
mod gnss;
pub mod psk;

use crate::at::*;
use crate::config::{SECURITY_TAG, SERVER_PORT, SERVER_URL};
use alloc_cortex_m::CortexMHeap;
use at_commands::parser::ParseError;
use coap_lite::error::MessageError;
use coap_lite::{CoapRequest, ContentFormat, RequestType};
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};
use defmt::info;
use embassy_nrf as _;
use embassy_time::TimeoutError;
use heapless::Vec;
use nrf_modem::{DtlsSocket, PeerVerification};
use serde::Serialize;
use {defmt_rtt as _, panic_probe as _};

/// Once flashed, comment this out along with the SPM entry in memory.x to eliminate flashing the SPM
/// more than once, and will speed up subsequent builds.  Or leave it and flash it every time
#[link_section = ".spm"]
#[used]
static SPM: [u8; 24052] = *include_bytes!("zephyr.bin");

/// Crate error types
#[derive(Debug)]
pub enum Error {
    Coap(MessageError),
    Json(serde_json::error::Error),
    NrfModem(nrf_modem::Error),
    Timeout(TimeoutError),
    ParseError(ParseError),
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

impl From<nrf_modem::Error> for Error {
    fn from(e: nrf_modem::Error) -> Self {
        Self::NrfModem(e)
    }
}

impl From<TimeoutError> for Error {
    fn from(e: TimeoutError) -> Self {
        Self::Timeout(e)
    }
}

impl From<ParseError> for Error {
    fn from(e: ParseError) -> Self {
        Self::ParseError(e)
    }
}

/// Payload to send over CoAP (Heapless Vec of Tanklevel Structs)
#[derive(Debug, Serialize)]
pub struct Payload<'a> {
    pub data: Vec<TankLevel, 6>,
    pub signal: i32,
    pub message: u8,
    pub timeouts: u8,
    location: &'a str,
}

/// Payload constructor
impl Payload<'_> {
    pub fn new() -> Self {
        Payload {
            data: Vec::new(),
            signal: 0,
            message: 0,
            timeouts: 0,
            location: "Reliability Test 3",
        }
    }
}

/// Structure to hold our individual measure data
#[derive(Debug, Serialize)]
pub struct TankLevel {
    pub value: u32,
    pub timestamp: u32,
    pub battery: u32,
}

/// TankLevel constructor
impl TankLevel {
    pub fn new(value: u32, timestamp: u32, battery: u32) -> Self {
        TankLevel {
            value,
            timestamp,
            battery,
        }
    }
}

/// Create CoAP request, serialize payload, and transimt data
/// request path can start with .s/ for LightDB Stream or .d/ LightDB State for Golioth IoT
pub async fn transmit_payload(payload: &mut Payload<'_>) -> Result<(), Error> {
    // Create our DTLS socket
    let socket = DtlsSocket::connect(
        SERVER_URL,
        SERVER_PORT,
        PeerVerification::Enabled,
        &[SECURITY_TAG],
    )
    .await?;
    info!("DTLS Socket connected");

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
    // info!("Payload: {:?}", Debug2Format(payload));
    // info!("JSON Byte Vec: {:?}", Debug2Format(&json));
    request.message.payload = json;

    socket.send(&request.message.to_bytes()?).await?;
    info!("Payload done");

    // The sockets would be dropped after the function call ends, but this explicit call allows them
    // to be dropped asynchronously
    info!("deactivate socket");
    socket.deactivate().await?;

    Ok(())
}

/// Convert sensor ADC value into tank level percentage
pub fn convert_to_tank_level(x: i16) -> u32 {
    let val = ((534 * x as u32) - 39_0634) / 10000;
    // info!("Tank Level: {}", &val);
    if val > 100 {
        100
    } else if val < 10 {
        10
    } else {
        val
    }
}

/// Convert ADC value into a milli-volt battery measurement
pub fn convert_to_mv(x: i16) -> u32 {
    // Stratus: V_bat measurement multiplier = 200/100
    // Icarus: V_bat measurement multiplier = 147/100
    ((x * (200 / 100)) as u32 * 3600) / 4096
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
