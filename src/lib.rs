#![no_main]
#![no_std]

use embassy_nrf as _;
use {defmt_rtt as _, panic_probe as _};

/// Terminates the application and makes `probe-run` exit with exit-code = 0
pub fn exit() -> ! {
    loop {
        cortex_m::asm::bkpt();
    }
}
