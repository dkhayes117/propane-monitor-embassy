use crate::Error;
use at_commands::parser::CommandParser;

/// Parse AT+CESQ command response and return a signal strength in dBm
/// Signal strength = -140 dBm + last int_parameter
pub async fn get_signal_strength() -> Result<i32, Error> {
    let command = nrf_modem::send_at::<32>("AT+CESQ").await?;

    let (_, _, _, _, _, mut signal) = CommandParser::parse(command.as_bytes())
        .expect_identifier(b"+CESQ:")
        .expect_int_parameter()
        .expect_int_parameter()
        .expect_int_parameter()
        .expect_int_parameter()
        .expect_int_parameter()
        .expect_int_parameter()
        .expect_identifier(b"\r\n")
        .finish()
        .unwrap();
    if signal != 255 {
        signal += -140;
    }
    Ok(signal)
}
