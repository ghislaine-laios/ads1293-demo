use ads1293_demo::driver::initialization::{Application3Lead, Initializer};
use ads1293_demo::driver::ADS1293;
use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::gpio::PinDriver;
use esp_idf_svc::hal::prelude::{Hertz, Peripherals};
use esp_idf_svc::hal::spi;
use esp_idf_svc::hal::spi::{config, SpiDeviceDriver, SpiDriver};

fn main() {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Hello, world!");

    let peripherals = Peripherals::take().expect("error when trying to take peripherals");

    let mut led_pin =
        PinDriver::output(peripherals.pins.gpio27).expect("error when trying to take led pin");
    led_pin
        .set_low()
        .expect("error when setting led pin to low");

    let spi = SpiDriver::new(
        peripherals.spi2,
        peripherals.pins.gpio14,
        peripherals.pins.gpio13,
        Some(peripherals.pins.gpio12),
        &spi::SpiDriverConfig::new(),
    )
    .expect("error when setting up the SPI interface (2)");

    let mut conifg = config::Config::default();
    conifg.baudrate = Hertz(2_000_000);
    let device = SpiDeviceDriver::new(spi, Some(peripherals.pins.gpio15), &conifg)
        .expect("error when setting up the SpiDevice of ads1293");

    let mut ads1293 = ADS1293::new(device);

    ads1293
        .init(Application3Lead)
        .expect("error when init the ads1293");

    log::info!("ADS1293 initialized.");

    loop {
        led_pin.set_high().unwrap();
        FreeRtos::delay_ms(5000);
        led_pin.set_low().unwrap();
        FreeRtos::delay_ms(200);
    }
}
