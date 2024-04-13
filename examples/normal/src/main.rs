use ads1293_demo::driver::initialization::Initializer;
use embassy_futures::select::select;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal;
use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::gpio;
use esp_idf_svc::hal::gpio::OutputPin;
use esp_idf_svc::hal::gpio::PinDriver;
use esp_idf_svc::hal::peripheral::Peripheral;
use esp_idf_svc::hal::prelude::*;
use esp_idf_svc::hal::spi;
use esp_idf_svc::hal::spi::SpiDeviceDriver;
use esp_idf_svc::hal::spi::SpiDriver;
use esp_idf_svc::hal::spi::SpiDriverConfig;
use esp_idf_svc::hal::task;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::timer::EspTaskTimerService;
use esp_idf_sys::esp;
use esp_idf_sys::esp_wifi_sta_get_rssi;
use futures_util::SinkExt;
use normal::communication::communication;
use normal::communication::connect_wifi;
use normal::communication::discover_service;
use normal::communication::setup_websocket;
use normal::communication::ConnectWifiPayload;
use normal::data;
use normal::settings::Settings;
use normal_data::Data;
use std::net::SocketAddr;
use std::thread;
use std::time::Duration;
use tokio_tungstenite::connect_async;

static DATA_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex,
    normal_data::Data,
    256,
> = Channel::<CriticalSectionRawMutex, Data, 256>::new();

fn main() -> anyhow::Result<()> {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    // eventfd is needed by our mio poll implementation.  Note you should set max_fds
    // higher if you have other code that may need eventfd.
    log::info!("Setting up eventfd...");
    let config = esp_idf_sys::esp_vfs_eventfd_config_t {
        max_fds: 4,
        ..Default::default()
    };
    esp! { unsafe { esp_idf_sys::esp_vfs_eventfd_register(&config) } }?;

    log::info!("hello world!");

    let settings = Settings::init().expect("failed to parse settings");

    let peripherals = Peripherals::take().expect("error when trying to take peripherals");

    let led_pin = peripherals.pins.gpio27;
    let mut led_pin = PinDriver::output(led_pin).expect("failed to take the led pin");
    led_pin.set_high().unwrap();
    FreeRtos::delay_ms(200);
    led_pin.set_low().unwrap();
    FreeRtos::delay_ms(200);
    led_pin.set_high().unwrap();

    let spi2 = peripherals.spi2;
    let sclk = peripherals.pins.gpio14;
    let sdo = peripherals.pins.gpio13;
    let sdi = peripherals.pins.gpio12;

    let ads1293_cs = peripherals.pins.gpio15;

    let _data_sender = DATA_CHANNEL.sender();
    let _data_receiver = DATA_CHANNEL.receiver();

    let connect_wifi_payload = ConnectWifiPayload {
        modem: peripherals.modem,
        sys_loop: EspSystemEventLoop::take().expect("cannot take the system event loop"),
        nvs: EspDefaultNvsPartition::take().expect("cannot take the default nvs partition"),
        timer_service: EspTaskTimerService::new().expect("cannot new the ESP task timer service"),
    };

    let wifi_fut = connect_wifi(
        settings,
        connect_wifi_payload.modem,
        connect_wifi_payload.sys_loop,
        connect_wifi_payload.nvs,
        connect_wifi_payload.timer_service,
    );

    let wifi = task::block_on(wifi_fut).unwrap();

    log::info!("the wifi has been setup");

    let (addr, port) =
        discover_service(settings.service.broadcast_port).expect("failed to discover service");

    let SocketAddr::V4(mut addr) = addr else {
        panic!("the service use ipv6 stack but it's not supported");
    };

    addr.set_port(port);

    log::info!("the service is discovered");

    let (tx, rx) = tokio::sync::mpsc::channel(2);

    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to setup tokio runtime")
        .block_on(async move {
            // We like blinking!
            tokio::spawn(led(led_pin));

            tokio::spawn(async move {
                loop {
                    let mut rssi: i32 = 0;
                    let r = unsafe { esp!(esp_wifi_sta_get_rssi(&mut rssi as *mut i32)) };
                    r.unwrap();
                    log::info!("current rssi: {}", rssi);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            });

            let mut socket = setup_websocket(addr).await;
            let communication_handle = tokio::spawn(async move {
                communication(&mut socket, rx).await;
                socket.flush().await.unwrap();
                log::error!("The websocket has closed.")
            });

            let spi = SpiDriver::new(spi2, sclk, sdo, Some(sdi), &SpiDriverConfig::new())
                .expect("failed when setting up the SPI interface (2)");

            let mut config = spi::SpiConfig::default();
            config.baudrate = Hertz(2_000_000);

            let device = SpiDeviceDriver::new(spi, Some(ads1293_cs), &config)
                .expect("failed when setting up the SpiDevice of ads1293");

            select(data::data(device, tx), communication_handle).await;
            log::error!("some routine exits!");
        });

    anyhow::Ok(())
}

async fn led<L: OutputPin>(mut led_pin: PinDriver<'static, L, gpio::Output>) {
    let mut next_low = true;

    loop {
        if next_low {
            led_pin.set_low().expect("failed to set the led pin to low");
        } else {
            led_pin
                .set_high()
                .expect("failed to set the led pin to high");
        }

        next_low = !next_low;

        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
