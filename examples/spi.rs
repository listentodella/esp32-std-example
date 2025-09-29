use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::spi::{config, SpiDeviceDriver, SpiDriver, SpiDriverConfig, SPI2},
    hal::units::*,
    // http::{client::EspHttpConnection, Method},
    // wifi::{AuthMethod, BlockingWifi, EspWifi},
};
fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    let peripherals = esp_idf_svc::hal::prelude::Peripherals::take().unwrap();
    let _sysloop = EspSystemEventLoop::take()?;
    let _fs = esp_idf_svc::io::vfs::MountedEventfs::mount(20)?;
    let tokio_runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let spi = peripherals.spi2;
    let sclk = peripherals.pins.gpio1;
    let serial_in = peripherals.pins.gpio2; // SDI
    let serial_out = peripherals.pins.gpio3; // SDO
    let cs = peripherals.pins.gpio4;

    let driver = SpiDriver::new::<SPI2>(
        spi,
        sclk,
        serial_out,
        Some(serial_in),
        &SpiDriverConfig::new(),
    )?;

    let config = config::Config::new()
        .baudrate(8.MHz().into())
        .data_mode(config::MODE_3);
    let mut spi = SpiDeviceDriver::new(&driver, Some(cs), &config)?;

    let mut data = [0u8; 5];

    tokio_runtime.block_on(async {
        loop {
            data[0] = 0x00 | 0x80;
            spi.transfer_in_place_async(&mut data).await.unwrap();
            log::info!("read_buffer: {:x?}", data);
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });

    Ok(())
}
