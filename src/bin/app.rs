#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use defmt::{error, info, unwrap};
use embassy_executor::Spawner;
use embassy_nrf::gpio::{Flex, Level, Output, OutputDrive};
use embassy_nrf::interrupt::{self, InterruptExt, Priority};
use embassy_nrf::pac::{REGULATORS, UARTE0, UARTE1};
// use embassy_nrf::pwm::{Prescaler, SimplePwm};
use embassy_nrf::saadc::{ChannelConfig, Config, Saadc};
use embassy_time::{Duration, Ticker, Timer};
use futures::StreamExt;
use nrf_modem::{ConnectionPreference, SystemMode};
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

    // // Disable UARTE for lower power consumption
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
            // If we get here, we have problems
            error!("app exited: {:?}", defmt::Debug2Format(&e));
            exit();
        }
    }
}

async fn run() -> Result<(), Error> {
    // Handle for device peripherals
    let mut p = embassy_nrf::init(Default::default());

    // Stratus: Disable on-board sensors for low power
    // Flex::new(&mut p.P0_25).set_as_disconnected();
    // Flex::new(&mut p.P0_28).set_as_disconnected();
    Flex::new(&mut p.P0_29).set_as_disconnected();

    // Configuration of ADC, over sample to reduce noise (8x)
    let adc_config = Config::default();
    // Oversample can only be used when you have a single channel
    // adc_config.oversample = Oversample::OVER8X;

    // Pin 14 can be used on both Stratus and Icarus boards for Analog Input
    let sensor_channel = ChannelConfig::single_ended(&mut p.P0_14);
    // Stratus: Pin 20 for V_bat measurement
    // Icarus: Pin 13 for V_bat measurement
    // let bat_channel = ChannelConfig::single_ended(&mut p.P0_20);

    let mut adc = Saadc::new(
        p.SAADC,
        interrupt::take!(SAADC),
        adc_config,
        [sensor_channel /*bat_channel*/],
    );
    adc.calibrate().await;
    info!("ADC Initialized");

    // Icarus: Has an eSIM and an External SIM.  Use Pin 8 to select: HIGH = eSIM, Low = External
    // Only change SIM selection while modem is off (AT+CFUN=1)
    // let _sim_select = Output::new(p.P0_08, Level::Low, OutputDrive::Standard);

    // Hall effect sensor power, must be High Drive to provide enough current (6 mA)
    let mut hall_effect = Output::new(p.P0_31, Level::Low, OutputDrive::Disconnect0HighDrive1);

    // Stratus: Pin 25 to control VBAT_MEAS_EN, Power must connect to V_Bat to measure correctly
    // Icarus: Pin 07 to disable battery charging circuit
    // let mut enable_bat_meas = Output::new(p.P0_25, Level::High, OutputDrive::Standard);
    // let _disable_charging = Output::new(p.P0_07, Level::High, OutputDrive::Standard);

    // Stratus: Pin 3 for blue LED power when data is being transmitted
    // Stratus: Pin 12 for blue LED power when data is being transmitted, (red: P_10, green: P_11)
    let mut led = Output::new(p.P0_03, Level::High, OutputDrive::Standard);

    // Initialize cellular modem
    unwrap!(
        nrf_modem::init(SystemMode {
            lte_support: true,
            nbiot_support: false,
            gnss_support: true,
            preference: ConnectionPreference::Lte,
        })
        .await
    );

    // Configure GPS settings
    // config_gnss().await?;

    // install PSK info for secure cloud connectivity
    install_psk_id_and_psk().await?;

    // Heapless buffer to hold our sample values before transmitting
    let mut payload = Payload::new();

    // Create our sleep timer (time between sensor measurements)
    let mut ticker = Ticker::every(Duration::from_secs(15));
    info!("Entering Loop");
    loop {
        let mut buf = [0; 1];

        // get_gnss_data().await?;

        // Power up the hall sensor: max power on time = 330us (wait for 500us to be safe)
        hall_effect.set_high();
        // enable_bat_meas.set_high();

        Timer::after(Duration::from_micros(500)).await;
        adc.sample(&mut buf).await;

        hall_effect.set_low();
        // enable_bat_meas.set_low();

        // Stratus: V_bat measurement multiplier = 200/100
        // Icarus: V_bat measurement multiplier = 147/100
        info!("Battery: {} ADC", &buf[0]);
        info!(
            "Battery: {} mV",
            (((&buf[0] * (200 / 100)) as u32 * 3600) / 4096)
        );

        payload
            .data
            .push(TankLevel::new(
                convert_to_tank_level(buf[0]),
                1987,
                ((&buf[0] * (200 / 100)) as u32 * 3600) / 4096,
            ))
            .unwrap();

        // Our payload data buff is full, send to the cloud, clear the buffer
        if payload.data.is_full() {
            info!("TankLevel: {}", core::mem::size_of::<TankLevel>());
            info!("Payload is full");

            // Visibly show that data is being sent
            led.set_low();

            // If timeout occurs, log a timeout and continue.
            if let Ok(_) =
                embassy_time::with_timeout(Duration::from_secs(30), transmit_payload(&mut payload))
                    .await
            {
                payload.timeouts = 0;

                info!("Transfer Complete");
            } else {
                payload.timeouts += 1;
                info!(
                    "Timeout has occurred {} time(s), data clear and start over",
                    payload.timeouts
                );
            }

            payload.data.clear();

            led.set_high();
        }
        info!("Ticker next()");
        ticker.next().await; // wait for next tick event
    }
}
