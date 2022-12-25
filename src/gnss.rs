use crate::Error;
use defmt::{info, Debug2Format};

pub async fn config_gnss() -> Result<(), Error> {
    // confgiure MAGPIO pins for GNSS
    info!("Configuring XMAGPIO pins for 1574-1577 MHz");
    nrf_modem::send_at::<0>("AT%XMAGPIO=1,0,0,1,1,1565,1586").await?;
    nrf_modem::send_at::<0>("AT%XCOEXO=1,1,1565,1586").await?;
    Ok(())
}

/// Function to retrieve GPS data from a single GNSS fix
pub async fn get_gnss_data() -> Result<(), Error> {
    let gnss = nrf_modem::Gnss::new().await?;
    let config = nrf_modem::GnssConfig::default();
    let mut iter = gnss.start_single_fix(config, 60)?;

    if let Some(x) = futures::StreamExt::next(&mut iter).await {
        info!("{:?}", Debug2Format(&x.unwrap()));
    }
    Ok(())
}
