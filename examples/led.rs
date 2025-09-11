use esp_idf_svc::{self, ws};
use smart_leds::{SmartLedsWrite, RGB, RGB8};
use ws2812_esp32_rmt_driver::Ws2812Esp32Rmt;

fn main() -> anyhow::Result<()> {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();
    let peripherals = esp_idf_svc::hal::prelude::Peripherals::take().unwrap();

    log::info!("Hello, world!");
    let channel = peripherals.rmt.channel0;
    let led_pin = peripherals.pins.gpio8;
    let mut ws2812 = Ws2812Esp32Rmt::new(channel, led_pin)?;
    loop {
        let pixels = std::iter::repeat(RGB8::new(30, 0, 0)).take(25);
        ws2812.write(pixels)?;
        log::info!("R");
        std::thread::sleep(std::time::Duration::from_millis(500));

        let pixels = std::iter::repeat(RGB8::new(0, 30, 0)).take(25);
        ws2812.write(pixels)?;
        log::info!("G");
        std::thread::sleep(std::time::Duration::from_millis(500));

        let pixels = std::iter::repeat(RGB8::new(0, 0, 30)).take(25);
        ws2812.write(pixels)?;
        log::info!("B");
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    Ok(())
}
