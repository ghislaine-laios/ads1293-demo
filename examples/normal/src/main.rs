use ads1293_demo::driver::initialization::Application3Lead;
use ads1293_demo::driver::initialization::Initializer;
use ads1293_demo::driver::registers;
use ads1293_demo::driver::registers::access::ReadFromRegister;
use ads1293_demo::driver::registers::DATA_STATUS;
use ads1293_demo::driver::ADS1293;
use anyhow::Context;
use embassy_futures::select::select;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::channel::Sender;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::gpio::OutputPin;
use esp_idf_svc::hal::gpio::PinDriver;
use esp_idf_svc::hal::peripheral::Peripheral;
use esp_idf_svc::hal::prelude::*;
use esp_idf_svc::hal::spi;
use esp_idf_svc::hal::spi::SpiDeviceDriver;
use esp_idf_svc::hal::spi::SpiDriver;
use esp_idf_svc::hal::spi::SpiDriverConfig;
use esp_idf_svc::hal::task;
use esp_idf_svc::hal::timer::Timer;
use esp_idf_svc::hal::timer::TimerConfig;
use esp_idf_svc::hal::timer::TimerDriver;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::timer::EspTaskTimerService;
use normal::communication::communication;
use normal::communication::ConnectWifiPayload;
use normal::settings::Settings;
use normal_data::Data;
use serde::de;

static DATA_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex,
    normal_data::Data,
    32,
> = Channel::<CriticalSectionRawMutex, Data, 32>::new();

fn main() -> anyhow::Result<()> {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("hello world!");
    Settings::init().expect("failed to parse settings");

    let peripherals = Peripherals::take().expect("error when trying to take peripherals");

    let led_pin = peripherals.pins.gpio27;
    let timer00 = peripherals.timer00;
    let timer01 = peripherals.timer01;

    let spi = SpiDriver::new(
        peripherals.spi2,
        peripherals.pins.gpio14,
        peripherals.pins.gpio13,
        Some(peripherals.pins.gpio12),
        &SpiDriverConfig::new(),
    )
    .expect("failed when setting up the SPI interface (2)");

    let ads1293_cs = peripherals.pins.gpio15;

    let data_sender = DATA_CHANNEL.sender();
    let data_receiver = DATA_CHANNEL.receiver();

    let thread_1 = std::thread::Builder::new()
        .name("thread 1".to_owned())
        .stack_size(4096)
        .spawn(move || {
            task::block_on(async move {
                select(
                    led(led_pin, timer00),
                    data(spi, ads1293_cs, timer01, data_sender),
                )
                .await;
            });
        })
        .context("failed to spawn the thread 1")?;

    let _thread_2 = std::thread::Builder::new()
        .name("thread 2".to_owned())
        .stack_size(8192)
        .spawn(move || {
            task::block_on(async move {
                let connect_wifi_payload = ConnectWifiPayload {
                    modem: peripherals.modem,
                    sys_loop: EspSystemEventLoop::take()
                        .expect("cannot take the system event loop"),
                    nvs: EspDefaultNvsPartition::take()
                        .expect("cannot take the default nvs partition"),
                    timer_service: EspTaskTimerService::new()
                        .expect("cannot new the ESP task timer service"),
                };

                communication(connect_wifi_payload, data_receiver).await
            })
        })
        .unwrap();

    thread_1
        .join()
        .map_err(|e| anyhow::Error::msg(format!("the thread 1 panicked: {:#?}", e)))?;

    anyhow::Ok(())
}

async fn led<L: OutputPin, T: Timer>(led_pin: L, timer: impl Peripheral<P = T>) {
    let mut led_pin = PinDriver::output(led_pin).expect("failed to take the led pin");

    let mut timer = TimerDriver::new(timer, &TimerConfig::new()).expect("failed to make the timer");

    let mut next_low = true;

    loop {
        if next_low {
            led_pin.set_low().expect("failed to set the led pin to low");
        } else {
            led_pin
                .set_high()
                .expect("failed to set the led pin to high");
        }

        timer
            .delay(timer.tick_hz())
            .await
            .expect("failed to delay using timer");

        next_low = !next_low;
    }
}

async fn data<T: Timer>(
    spi: SpiDriver<'_>,
    cs: impl OutputPin,
    timer: impl Peripheral<P = T>,
    data_sender: Sender<'_, CriticalSectionRawMutex, Data, 32>,
) {
    let mut config = spi::SpiConfig::default();
    config.baudrate = Hertz(2_000_000);

    let device = SpiDeviceDriver::new(spi, Some(cs), &config)
        .expect("failed when setting up the SpiDevice of ads1293");

    let mut ads1293 = ADS1293::new(device);

    ads1293
        .init(Application3Lead)
        .expect("failed to init the ads1293");

    log::info!("ADS1293 initialized.");

    let main_config = ads1293
        .read(registers::CONFIG)
        .expect("failed to read the main config");

    log::info!("main_config: {:#?}", main_config);

    let mut timer = TimerDriver::new(
        timer,
        &TimerConfig {
            ..Default::default()
        },
    )
    .expect("failed to create the data timer");

    let loop_back_mode_config = ads1293
        .read(registers::CH_CNFG)
        .expect("fail to read loop back mode config");
    log::info!("loop_back_mode_config: {:#?}", loop_back_mode_config);

    let mut counter = 0;
    loop {
        timer
            .delay(timer.tick_hz() / 60)
            .await
            .expect("failed to delay using timer");

        let data_status = ads1293.read(DATA_STATUS).expect("fail to read data status");

        let data = ads1293
            .stream_one()
            .expect("failed to read data under stream mode");
        log::trace!("data: {:?}", data);

        counter += 1;
        if counter == 300 {
            counter = 0;
            log::info!("data status: {:#?}", data_status);
        }
    }
}
