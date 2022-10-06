#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]


use defmt::{info, unwrap};
use embassy_executor::Spawner;
use embassy_nrf::gpio::{Flex, Level, Output, OutputDrive};
use embassy_nrf::interrupt::{self, InterruptExt, Priority};
// use embassy_nrf::pwm::{Prescaler, SimplePwm};
use embassy_nrf::saadc::{ChannelConfig, Config, Oversample, Saadc};
use embassy_time::{Duration, Ticker, Timer};
use futures::StreamExt;
use nrf_modem::{ConnectionPreference, SystemMode};
use nrf_modem::lte_link::LteLink;
use propane_monitor_embassy as _;
use propane_monitor_embassy::{Dtls, TankLevel};


#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    // Set up the interrupts for the modem
    let egu1 = embassy_nrf::interrupt::take!(EGU1);
    egu1.set_priority(Priority::P4);
    egu1.set_handler(|_| {
        nrf_modem::application_irq_handler();
        cortex_m::asm::sev();
    });
    egu1.enable();

    let ipc = embassy_nrf::interrupt::take!(IPC);
    ipc.set_priority(Priority::P0);
    ipc.set_handler(|_| {
        nrf_modem::ipc_irq_handler();
        cortex_m::asm::sev();
    });
    ipc.enable();

    let regulators: embassy_nrf::pac::REGULATORS = unsafe { core::mem::transmute(()) };
    regulators.dcdcen.modify(|_, w| w.dcdcen().enabled());

    // Disable UARTE for lower power consumption
    let uarte0 = unsafe{ &*embassy_nrf::pac::UARTE0::PTR};
    let uarte1 = unsafe{ &*embassy_nrf::pac::UARTE1::PTR};
    uarte0.enable.write(|w| w.enable().disabled());
    uarte1.enable.write(|w| w.enable().disabled());

    // Run our sampling program
    run().await;

    propane_monitor_embassy::exit();
}

async fn run() {
    // Handle for device peripherals
    let mut p = embassy_nrf::init(Default::default());

    // Disable on-board sensors for low power
    Flex::new(&mut p.P0_29).set_as_disconnected();

    // Create our sleep timer (time between sensor measurements)
    let mut ticker = Ticker::every(Duration::from_secs(15));

    // Heapless buffer to hold our sample values before transmitting
    let mut tank_level = TankLevel::new();

    // Configuration of ADC
    let mut adc_config = Config::default();
    adc_config.oversample = Oversample::OVER8X;

    let channel_config = ChannelConfig::single_ended(&mut p.P0_14);
    let mut adc = Saadc::new(
        p.SAADC,
        interrupt::take!(SAADC),
        adc_config,
        [channel_config],
    );

    // Hall effect sensor power, must be High Drive to provide enough current (6 mA)
    let mut hall_effect = Output::new(
        p.P0_31,
        Level::Low,
        OutputDrive::Disconnect0HighDrive1
    );

    // Initialize modem
    unwrap!(
        nrf_modem::init( SystemMode {
            lte_support: true,
            nbiot_support: false,
            gnss_support: true,
            preference: ConnectionPreference::Lte,
        }).await
    );
    //
    // // Create our LTE Link
    // let link = LteLink::new().await.unwrap();
    // let mut dtls = Dtls::new();

    loop {
        let mut buf = [0; 1];

        // Power up the hall sensor: max power on time = 330us
        hall_effect.set_high();
        Timer::after(Duration::from_micros(500)).await;

        adc.sample(&mut buf).await;

        hall_effect.set_low();

        tank_level.data.push(buf[0]).unwrap();

        // Our payload data buff is full, send to the cloud, clear the buffer, disconnect socket
        if tank_level.data.is_full() {
            for val in &tank_level.data{
                info!("{}", val);
            }
            // dtls.transmit_payload(&tank_level).unwrap().await;
            tank_level.data.clear();
        }

        ticker.next().await; // wait for next tick event
    }
}
