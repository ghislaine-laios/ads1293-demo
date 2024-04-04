use ads1293_demo::driver::initialization::Application3Lead;
use ads1293_demo::driver::initialization::Initializer;
use ads1293_demo::driver::registers;
use ads1293_demo::driver::registers::access::ReadFromRegister;
use ads1293_demo::driver::registers::DataRegister;
use ads1293_demo::driver::registers::DATA_STATUS;
use ads1293_demo::driver::ADS1293;
use embassy_futures::select::select;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embedded_hal::spi::SpiDevice;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::gpio::OutputPin;
use esp_idf_svc::hal::gpio::PinDriver;
use esp_idf_svc::hal::prelude::*;
use esp_idf_svc::hal::spi;
use esp_idf_svc::hal::spi::SpiDeviceDriver;
use esp_idf_svc::hal::spi::SpiDriver;
use esp_idf_svc::hal::spi::SpiDriverConfig;
use esp_idf_svc::hal::task;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::timer::EspTaskTimerService;
use esp_idf_sys::esp;
use esp_idf_sys::esp_timer_get_time;
use futures_util::SinkExt;
use normal::communication::communication;
use normal::communication::connect_wifi;
use normal::communication::discover_service;
use normal::communication::setup_websocket;
use normal::communication::ConnectWifiPayload;
use normal::settings::Settings;
use normal_data::Data;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::sync::mpsc::error::TrySendError;

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

    let _wifi = task::block_on(wifi_fut).unwrap();

    log::info!("the wifi has been setup");

    let (addr, port) =
        discover_service(settings.service.broadcast_port).expect("failed to discover service");

    let SocketAddr::V4(mut addr) = addr else {
        panic!("the service use ipv6 stack but it's not supported");
    };

    addr.set_port(port);

    log::info!("the service is discovered");

    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to setup tokio runtime")
        .block_on(async move {
            // We like blinking!
            tokio::spawn(led(led_pin));

            let mut socket = setup_websocket(addr).await;
            log::info!("the websocket client has been setup");

            let (tx, rx) = tokio::sync::mpsc::channel(2);

            let spi = SpiDriver::new(spi2, sclk, sdo, Some(sdi), &SpiDriverConfig::new())
                .expect("failed when setting up the SPI interface (2)");

            let mut config = spi::SpiConfig::default();
            config.baudrate = Hertz(2_000_000);

            let device = SpiDeviceDriver::new(spi, Some(ads1293_cs), &config)
                .expect("failed when setting up the SpiDevice of ads1293");

            let ws = async {
                communication(&mut socket, rx).await;
                socket.flush().await.unwrap();
            };

            select(data(device, tx), ws).await;
            log::error!("some routine exits!");
        });

    anyhow::Ok(())
}

async fn led<L: OutputPin>(led_pin: L) {
    let mut led_pin = PinDriver::output(led_pin).expect("failed to take the led pin");

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

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn data(device: impl SpiDevice, data_sender: tokio::sync::mpsc::Sender<Data>) {
    let mut ads1293 = ADS1293::new(device);

    ads1293
        .init(Application3Lead)
        .expect("failed to init the ads1293");

    log::info!("ADS1293 initialized.");

    let main_config = ads1293
        .read(registers::CONFIG)
        .expect("failed to read the main config");

    log::info!("main_config: {:#?}", main_config);

    let loop_back_mode_config = ads1293
        .read(registers::CH_CNFG)
        .expect("fail to read loop back mode config");
    log::info!("loop_back_mode_config: {:#?}", loop_back_mode_config);

    let mut id = 0;
    let mut perf_counter = 0;
    let mut fail_counter = 60;
    let mut start_time = unsafe { esp_timer_get_time() };
    let frame_duration = Duration::from_secs_f32(1.0 / 60.0);
    let mut sleep_deadline = tokio::time::Instant::now();
    'out: loop {
        tokio::time::sleep_until(sleep_deadline).await;
        perf_counter += 1;
        sleep_deadline = sleep_deadline.checked_add(frame_duration).unwrap();

        let _data_status = ads1293.read(DATA_STATUS).expect("fail to read data status");

        let data_vec = ads1293
            .stream_one()
            .expect("failed to read data under stream mode");
        log::trace!("data: {:?}", data_vec);

        for data in data_vec {
            let DataRegister::DATA_CH1_ECG(data) = data else {
                continue;
            };

            if let Err(e) = data_sender.try_send(Data {
                id,
                value: data.into(),
            }) {
                match e {
                    // I drop the data directly to ensure that
                    // the data's ID remains reliable for timing purposes.
                    TrySendError::Full(_data) => {
                        if fail_counter == 60 {
                            log::warn!("the data channel is full. the data will be dropped.");
                            fail_counter = 0;
                        }

                        fail_counter += 1;
                    }
                    TrySendError::Closed(_) => break 'out,
                }
            }

            break;
        }

        id += 1;

        if perf_counter == 600 {
            let end_time = unsafe { esp_timer_get_time() };
            let span = end_time - start_time;
            start_time = end_time;
            log::info!("last 600 frame: {} ms", span / 1000);
            perf_counter = 0;
        }
    }
}
