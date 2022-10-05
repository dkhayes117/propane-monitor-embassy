#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

// use core::mem;
use embassy_executor::Spawner;
use embassy_nrf::gpio::{Flex};

// use embassy_nrf::pac::UARTE0;
// use embassy_nrf::uarte::Uarte;
use propane_monitor_embassy as _;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut p = embassy_nrf::init(Default::default());

    // Disconnect pin 29 for power savings
    Flex::new(&mut p.P0_29).set_as_disconnected();

    // Disable UARTE0 and UARTE1 for power savings
    let uarte0 = unsafe{ &*embassy_nrf::pac::UARTE0::PTR };
    let uarte1 = unsafe{ &*embassy_nrf::pac::UARTE1::PTR };

    uarte0.enable.write( |w| w.enable().disabled() );
    uarte1.enable.write( |w| w.enable().disabled() );

    // let regulators = unsafe { &*embassy_nrf::pac::REGULATORS::PTR };
    // regulators.systemoff.write(|w| w.systemoff().enable());
    // loop {wfe()}
}


