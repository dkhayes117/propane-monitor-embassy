#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use defmt::{info, unwrap};
use embassy_executor::Spawner;
use embassy_nrf::gpio::{Flex, Level, Output, OutputDrive};
use embassy_nrf::interrupt::{self, InterruptExt, Priority};
use embassy_nrf::pac::{REGULATORS, UARTE0, UARTE1};
// use embassy_nrf::pwm::{Prescaler, SimplePwm};
use embassy_nrf::saadc::{ChannelConfig, Config, Oversample, Saadc};
use embassy_time::{Duration, Ticker, Timer};
use futures::StreamExt;
use nrf_modem::{ConnectionPreference, SystemMode};
// use propane_monitor_embassy as _;
use propane_monitor_embassy::*;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    // Set up the interrupts for the modem
    let egu1 = interrupt::take!(EGU1);
    egu1.set_priority(Priority::P4);
    egu1.set_handler(|_| {
        nrf_modem::application_irq_handler();
        cortex_m::asm::sev();
    });
    egu1.enable();

    let ipc = interrupt::take!(IPC);
    ipc.set_priority(Priority::P0);
    ipc.set_handler(|_| {
        nrf_modem::ipc_irq_handler();
        cortex_m::asm::sev();
    });
    ipc.enable();

    // dcdcen must be enabled before the modem is started, not after
    // Enabling DCDC mode will allow modem to automatically switch between DCDC mode and LDO
    // mode for greatest power efficiency
    let regulators: REGULATORS = unsafe { core::mem::transmute(()) };
    regulators.dcdcen.modify(|_, w| w.dcdcen().enabled());

    // Disable UARTE for lower power consumption
    let uarte0: UARTE0 = unsafe { core::mem::transmute(()) };
    let uarte1: UARTE1 = unsafe { core::mem::transmute(()) };
    uarte0.enable.write(|w| w.enable().disabled());
    uarte1.enable.write(|w| w.enable().disabled());

    // Initialize heap data
    alloc_init();

    // Run our sampling program, will not return unless an error occurs
    match run().await {
        Ok(()) => exit(),
        Err(e) => {
            // If we get here, we have problems, reboot device
            info!("app exited: {:?}", defmt::Debug2Format(&e));
            exit();
        }
    }
}

async fn run() -> Result<(), Error> {
    // Handle for device peripherals
    let mut p = embassy_nrf::init(Default::default());

    // Disable on-board sensors for low power
    Flex::new(&mut p.P0_29).set_as_disconnected();

    // Create our sleep timer (time between sensor measurements)
    let mut ticker = Ticker::every(Duration::from_secs(15));

    // Configuration of ADC, over sample to reduce noise (8x)
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
    let mut hall_effect = Output::new(p.P0_31, Level::Low, OutputDrive::Disconnect0HighDrive1);

    // blue LED to blink when data is being transmitted on Conexio Stratus
    let mut blue_led = Output::new(p.P0_03, Level::High, OutputDrive::Standard);

    // Initialize modem
    unwrap!(
        nrf_modem::init(SystemMode {
            lte_support: true,
            nbiot_support: false,
            gnss_support: true,
            preference: ConnectionPreference::Lte,
        })
        .await
    );

    // install PSK info for secure cloud connectivity
    install_psk_id_and_psk().await?;

    // Heapless buffer to hold our sample values before transmitting
    let mut payload = Payload::new();

    loop {
        let mut buf = [0; 1];

        // gnss_data().await?;

        // Power up the hall sensor: max power on time = 330us
        hall_effect.set_high();
        Timer::after(Duration::from_micros(500)).await;

        adc.sample(&mut buf).await;

        hall_effect.set_low();

        payload.data.push(TankLevel::new(buf[0], 1987)).unwrap();

        // Our payload data buff is full, send to the cloud, clear the buffer
        if payload.data.is_full() {
            // for val in &tank_level.data {
            //     info!("ADC: {}", val);
            // }
            // info!("TankLevel: {}", core::mem::size_of::<TankLevel>());

            // Visibly show that data is being sent
            blue_led.set_low();

            info!("Transmitting data over CoAP");
            // If timeout occurs log a timeout and continue.
            if let Err(e) = embassy_time::with_timeout(Duration::from_secs(30), transmit_payload(&payload))
                .await {
                payload.timeouts += 1;
                info!("{} has occurred {} time(s), data clear and start over", e, payload.timeouts);
            } else {
                payload.timeouts = 0;
            }

            payload.data.clear();

            blue_led.set_high();
        }

        ticker.next().await; // wait for next tick event
    }
}
