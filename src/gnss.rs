use defmt::{info,Debug2Format};
use crate::Error;

pub async fn config_gnss() -> Result<(), Error> {
    // confgiure MAGPIO pins for GNSS
    info!("Configuring XMAGPIO pins for 1574-1577 MHz");
    nrf_modem::at::send_at::<0>("AT%XMAGPIO=1,0,0,1,1,1574,1577").await?;
    nrf_modem::at::send_at::<0>("AT%XCOEXO=1,1,1574,1577").await?;
    Ok(())
}

/// Function to retrieve GPS data from a single GNSS fix
pub async fn get_gnss_data() -> Result<(), Error> {
    let mut gnss = nrf_modem::gnss::Gnss::new().await?;
    let config = nrf_modem::gnss::GnssConfig::default();
    let mut iter = gnss.start_single_fix(config)?;

    if let Some(x) = futures::StreamExt::next(&mut iter).await {
        info!("{:?}", Debug2Format(&x.unwrap()));
    }
    Ok(())
}