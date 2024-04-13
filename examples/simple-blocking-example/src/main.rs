use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{
        delay::FreeRtos,
        gpio::PinDriver,
        peripherals::Peripherals,
        spi::{self, SpiDeviceDriver, SpiDriver, SpiDriverConfig},
        units::Hertz,
    },
    nvs::EspDefaultNvsPartition,
    timer::EspTaskTimerService,
    wifi::{BlockingWifi, EspWifi},
    ws::client::{EspWebSocketClient, EspWebSocketClientConfig},
};
use normal_data::{Data, PUSH_DATA_ENDPOINT};
use simple_blocking_example::{
    data::{init_ads1293, retrieve_data_once},
    led::{in_program_blink, start_program_blink},
    settings::Settings,
    transport::discover_service,
    wifi::connect_wifi,
};

fn main() {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Hello, world!");

    let settings = Settings::init().expect("failed to parse settings");

    let peripherals = Peripherals::take().expect("error when trying to take peripherals");
    let sys_loop = EspSystemEventLoop::take().unwrap();
    let nvs = EspDefaultNvsPartition::take().unwrap();

    let mut led_pin = PinDriver::output(peripherals.pins.gpio27).unwrap();
    start_program_blink(&mut led_pin);

    let wifi = {
        let mut wifi = BlockingWifi::wrap(
            EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs)).unwrap(),
            sys_loop,
        )
        .unwrap();

        connect_wifi(settings, &mut wifi);

        let ip_info = wifi.wifi().sta_netif().get_ip_info().unwrap();

        log::info!("Wifi DHCP info: {:?}", ip_info);

        wifi
    };

    let mut ws_client = {
        let socket_addr = discover_service(settings.service.broadcast_port).unwrap();
        let url = format!("ws://{}{}", socket_addr, PUSH_DATA_ENDPOINT);

        log::info!("Setting up the websocket client to connect to {}", url);

        EspWebSocketClient::new(
            &url,
            &EspWebSocketClientConfig {
                network_timeout_ms: Duration::from_millis(100),
                reconnect_timeout_ms: Duration::from_millis(10),
                ..Default::default()
            },
            Duration::from_millis(500),
            |_| {},
        )
        .unwrap()
    };

    let mut ads1293 = {
        let spi2 = peripherals.spi2;
        let sclk = peripherals.pins.gpio14;
        let sdo = peripherals.pins.gpio13;
        let sdi = peripherals.pins.gpio12;
        let ads1293_cs = peripherals.pins.gpio15;

        let spi = SpiDriver::new(spi2, sclk, sdo, Some(sdi), &SpiDriverConfig::new())
            .expect("failed when setting up the SPI interface (2)");

        let spi = SpiDeviceDriver::new(
            spi,
            Some(ads1293_cs),
            &spi::SpiConfig {
                baudrate: Hertz(2_000_000),
                ..Default::default()
            },
        )
        .unwrap();

        let ads1293 = init_ads1293(spi);

        log::info!("ADS1293 has been initialized.");

        ads1293
        // Arc::new(Mutex::new(ads1293))
    };

    let timer_service = EspTaskTimerService::new().unwrap();

    let timer = {
        let mut id = 0;
        timer_service.timer(move || {
            let data = retrieve_data_once(&mut ads1293);

            id += 1;
            let data = Data { id, value: data };
            log::debug!("{:?}", data);

            let str = serde_json::to_string(&data).unwrap();
            let str = str.as_bytes();
            if !ws_client.is_connected() {
                return;
            }
            ws_client
                .send(esp_idf_svc::ws::FrameType::Text(false), str)
                .unwrap();
        })
    }
    .unwrap();
    timer.every(Duration::from_millis(300)).unwrap();

    let mut is_pin_high = true;
    loop {
        in_program_blink(&mut led_pin, &mut is_pin_high);
        FreeRtos::delay_ms(1000);
        log::debug!("wifi: {:?}", wifi.is_up().unwrap())
    }
}
