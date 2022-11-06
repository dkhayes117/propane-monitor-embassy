#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_nrf::gpio::{Level, Output, OutputDrive};
use embassy_nrf::interrupt;
use embassy_nrf::pwm::{Prescaler, SimplePwm};
use embassy_nrf::saadc::{ChannelConfig, Config, Oversample, Saadc};
use embassy_time::{Duration, Timer};
use propane_monitor_embassy as _;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut p = embassy_nrf::init(Default::default());
    let mut pwm = SimplePwm::new_1ch(p.PWM0, p.P0_10);

    let mut adc_config = Config::default();
    adc_config.oversample = Oversample::OVER8X;

    let channel_config = ChannelConfig::single_ended(&mut p.P0_14);
    let mut adc = Saadc::new(
        p.SAADC,
        interrupt::take!(SAADC),
        adc_config,
        [channel_config],
    );

    adc.calibrate().await;
    info!("ADC Initialized");

    let _hall_effect = Output::new(p.P0_31, Level::High, OutputDrive::Disconnect0HighDrive1);

    let mut buf = [0; 1];

    // most servos require 50hz or 20ms period
    // set_period can only set down to 125khz so we cant use it directly
    // Div128 is 125khz or 0.000008s or 0.008ms, 20/0.008 = 2500 which is top value
    pwm.set_prescaler(Prescaler::Div128);
    pwm.set_max_duty(2500);
    info!("pwm initialized!");

    // Array of tuples holding a calibrated duty_cycle for each gauge level
    // 1ms 0deg (1/.008=125), 1.5ms 90deg (1.5/.008=187.5), 2ms 180deg (2/.008=250),
    let positions: [(u16, u16); 13] = [
        (5, 111),
        (10, 122),
        (15, 134),
        (20, 144),
        (25, 154),
        (30, 162),
        (40, 176),
        (50, 189),
        (60, 203),
        (70, 217),
        (80, 234),
        (85, 244),
        (88, 250),
    ];

    Timer::after(Duration::from_millis(5000)).await;

    for _ in 1..11 {
        for (level, duty) in positions.iter() {
            // poor mans inverting, subtract our value from max_duty
            pwm.set_duty(0, 2500 - *duty);
            Timer::after(Duration::from_millis(3000)).await;

            adc.sample(&mut buf).await;
            info!("Gauge Level: {}%, adc: {=i16}, conversion: {=u32}"
                , level, &buf[0], convert_to_tank_level(buf[0])
            );
        }
    }
    propane_monitor_embassy::exit();
}

/// Convert sensor ADC value into tank level percentage
fn convert_to_tank_level(x: i16) -> u32 {
    let val = ((534 * x as u32) - 39_0634) / 10000;
    info!("Tank Level: {}", &val);
    if val > 100 {
        100
    } else if val < 10 {
        10
    } else {
        val
    }
}