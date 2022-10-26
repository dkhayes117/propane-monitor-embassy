#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use embassy_executor::Spawner;
use embassy_nrf::gpio::{Level, Output, OutputDrive};
use embassy_nrf::pwm::{Prescaler, SimplePwm};
use embassy_time::{Duration, Timer};
use propane_monitor_embassy as _;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_nrf::init(Default::default());
    let mut led_pwm = SimplePwm::new_1ch(p.PWM0, p.P0_03);
    led_pwm.set_prescaler(Prescaler::Div1);
    led_pwm.set_max_duty(32767);
    led_pwm.set_duty(0,0);

    loop {
        led_pwm.set_duty(0,0);
        Timer::after(Duration::from_millis(500)).await;
        led_pwm.set_duty(0,32767);
        Timer::after(Duration::from_millis(500)).await;
    }
}
