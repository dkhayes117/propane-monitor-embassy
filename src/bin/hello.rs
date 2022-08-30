#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use embassy_executor::Spawner;
use propane_monitor_embassy as _;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    defmt::info!("Hello, world!");

    propane_monitor_embassy::exit();
}
