use ads1293_demo::driver::initialization::Application3Lead;
use ads1293_demo::driver::initialization::Initializer;
use ads1293_demo::driver::registers;
use ads1293_demo::driver::registers::access::ReadFromRegister;
use ads1293_demo::driver::ADS1293;
use anyhow::Context;
use embassy_futures::join::join;
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
use esp_idf_svc::ws::client::EspWebSocketClient;

pub mod settings;

fn main() -> anyhow::Result<()> {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("hello world!");

    let stack_size = 7000;
    let thread_1 = std::thread::Builder::new()
        .name("thread 1".to_owned())
        .stack_size(stack_size)
        .spawn(move || {
            task::block_on(async {
                let peripherals =
                    Peripherals::take().expect("error when trying to take peripherals");

                let led_pin = peripherals.pins.gpio27;
                let timer00 = peripherals.timer00;

                let spi = SpiDriver::new(
                    peripherals.spi2,
                    peripherals.pins.gpio14,
                    peripherals.pins.gpio13,
                    Some(peripherals.pins.gpio12),
                    &SpiDriverConfig::new(),
                )
                .expect("failed when setting up the SPI interface (2)");

                let ads1293_cs = peripherals.pins.gpio15;

                join(led(led_pin, timer00), data(spi, ads1293_cs)).await;
            });
        })
        .context("failed to spawn the thread 1")?;

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

async fn data(spi: SpiDriver<'_>, cs: impl OutputPin) {
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
}

async fn communication() {
    
}

async fn connect_wifi() {
    
}
