use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::peripheral,
    hal::spi::{config, SpiDeviceDriver, SpiDriver, SpiDriverConfig, SPI2},
    hal::units::*,
    http::{client::EspHttpConnection, Method},
    wifi::{AuthMethod, BlockingWifi, EspWifi},
};
use futures_util::SinkExt;
use peripheral_bridge::pb::{msg::*, prost::Message};

fn wifi(
    ssid: &str,
    passwd: &str,
    modem: impl peripheral::Peripheral<P = esp_idf_svc::hal::modem::Modem> + 'static,
    sysloop: EspSystemEventLoop,
) -> anyhow::Result<Box<EspWifi<'static>>> {
    let mut auth_method = AuthMethod::WPA2Personal;
    if ssid.is_empty() {
        anyhow::bail!("Missing WiFi name")
    }
    if passwd.is_empty() {
        auth_method = AuthMethod::None;
        log::info!("No password provided, using open network");
    }

    let mut esp_wifi = EspWifi::new(modem, sysloop.clone(), None)?;
    let mut wifi = BlockingWifi::wrap(&mut esp_wifi, sysloop)?;

    wifi.set_configuration(&esp_idf_svc::wifi::Configuration::Client(
        esp_idf_svc::wifi::ClientConfiguration {
            ssid: ssid
                .try_into()
                .expect("Could not parse the given SSID into WiFi config"),
            password: passwd
                .try_into()
                .expect("Could not parse the given password into WiFi config"),
            auth_method,
            ..Default::default()
        },
    ))?;
    wifi.start()?;
    log::info!("Connecting wifi...");
    wifi.connect()?;
    log::info!("Waiting for DHCP lease...");
    wifi.wait_netif_up()?;
    let ip_info = wifi.wifi().sta_netif().get_ip_info()?;
    log::info!("Wifi DHCP info: {:?}", ip_info);

    Ok(Box::new(esp_wifi))
}

async fn http_get(url: &str) -> anyhow::Result<EspHttpConnection> {
    let configuration = esp_idf_svc::http::client::Configuration::default();
    let mut conn = EspHttpConnection::new(&configuration)?;
    conn.initiate_request(Method::Get, url, &[])?;

    conn.initiate_response()?;
    Ok(conn)
}

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    let peripherals = esp_idf_svc::hal::prelude::Peripherals::take().unwrap();
    let sysloop = EspSystemEventLoop::take()?;
    let _fs = esp_idf_svc::io::vfs::MountedEventfs::mount(20)?;
    //SSID=wifi_name PASSWD=xxx SERVER_URL=ws://host_ip:8765 cargo run --example web
    let ssid: Option<&str> = option_env!("SSID");
    let passwd: Option<&str> = option_env!("PASSWD");
    const SERVER_URL: Option<&str> = option_env!("SERVER_URL");

    let tokio_runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    // Configure SPI
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

    // Configures the button
    let mut button = esp_idf_svc::hal::gpio::PinDriver::input(peripherals.pins.gpio9)?;
    button.set_pull(esp_idf_svc::hal::gpio::Pull::Up)?;
    button.set_interrupt_type(esp_idf_svc::hal::gpio::InterruptType::PosEdge)?;
    tokio_runtime.spawn(async move {
        loop {
            let _ = button.wait_for_falling_edge().await;
            log::info!("Button  pressed {:?}", button.get_level());
            unsafe { esp_idf_svc::sys::esp_restart() }
        }
    });

    let _wifi = wifi(
        ssid.unwrap(),
        passwd.unwrap_or_default(),
        peripherals.modem,
        sysloop,
    )
    .unwrap();

    tokio_runtime.block_on(async {
        log::info!("start HTTP GET request");
        match http_get("http://httpbin.org/get").await {
            Ok(_) => log::info!("HTTP GET request SUCCESS"),
            Err(_) => log::error!("HTTP GET request FAILURE"),
        }
    });

    if let Some(url) = SERVER_URL {
        log::info!("start WebSocket task to {}", url);
        tokio_runtime.block_on(ws_task(url))?;
    } else {
        log::warn!("No SERVER_URL provided, skipping WebSocket task");
    }

    tokio_runtime.block_on(async {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            log::info!("tick");
        }
    });

    Ok(())
}

async fn ws_task(url: &str) -> anyhow::Result<()> {
    use futures_util::StreamExt;

    let (mut ws_stream, _) = tokio_websockets::ClientBuilder::new()
        .uri(url)?
        .connect()
        .await?;
    log::info!("WebSocket connected to {}", url);

    let mut i = 0;
    while let Some(msg) = ws_stream.next().await {
        match msg {
            Ok(msg) => {
                if let Some(text) = msg.as_text() {
                    log::info!("Received message: {}", text);
                    // tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    ws_stream
                        .send(tokio_websockets::Message::text(format!(
                            "hello websocket from ESP32 to Server: {i}"
                        )))
                        .await?;
                    i += 1;
                    if i > 10 {
                        log::info!("Done, closing WebSocket connection");
                        break;
                    }
                } else {
                    // let ws_msg = msg.clone();
                    // log::info!("Received message: {:?}", ws_msg);
                    let rx_msgs = msg.into_payload().to_vec();
                    let rx_msgs = MsgBatch::decode(rx_msgs.as_slice()).unwrap();
                    // log::info!("Received message: {:?}", rx_msgs);
                    for msg in rx_msgs.msgs {
                        for seq in msg.seqs {
                            match Operation::try_from(seq.operation).unwrap() {
                                Operation::Ack => {
                                    log::info!("Received Ack operation");
                                }
                                Operation::Read => {
                                    log::info!("Received Read operation");
                                }
                                Operation::Write => {
                                    log::info!("Received Write operation");
                                }
                                Operation::Transfer => {
                                    log::info!("Received Transfer operation");
                                }
                            }
                        }
                    }

                    // ws_stream
                    //     .send(tokio_websockets::Message::binary(msg.into_payload()))
                    //     .await?;
                }
            }
            Err(e) => {
                log::error!("WebSocket error: {}", e);
                break;
            }
        }
    }

    Ok(())
}
