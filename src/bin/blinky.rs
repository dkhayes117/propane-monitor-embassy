#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_nrf::gpio::{Level, Output, OutputDrive};
use embassy_time::{Duration, Timer};
use propane_monitor_embassy as _;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_nrf::init(Default::default());
    let mut led = Output::new(p.P0_30, Level::Low, OutputDrive::Disconnect0HighDrive1);

    loop {
        info!("blink loop");
        led.set_high();
        Timer::after(Duration::from_millis(1000)).await;
        info!("{}", led.is_set_high());
        led.set_low();
        Timer::after(Duration::from_millis(1000)).await;
        info!("{}", led.is_set_high());
    }
}
