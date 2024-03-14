use ads1293_demo::driver::initialization::{Application3Lead, Initializer};
use ads1293_demo::driver::registers::access::ReadFromRegister;
use ads1293_demo::driver::registers::DATA_STATUS;
use ads1293_demo::driver::{registers, ADS1293};
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

    let mut config = config::Config::default();
    config.baudrate = Hertz(2_000_000);
    let device = SpiDeviceDriver::new(spi, Some(peripherals.pins.gpio15), &config)
        .expect("error when setting up the SpiDevice of ads1293");

    let mut ads1293 = ADS1293::new(device);

    ads1293
        .init(Application3Lead)
        .expect("error when init the ads1293");

    log::info!("ADS1293 initialized.");

    let main_config = ads1293
        .read(registers::CONFIG)
        .expect("fail to read main config");

    log::info!("main_config: {:#?}", main_config);

    loop {
        let data_status = ads1293.read(DATA_STATUS).expect("fail to read data status");
        log::info!("data_status: {:#?}", data_status);
        let loop_back_mode_config = ads1293
            .read(registers::CH_CNFG)
            .expect("fail to read loop back mode config");
        log::info!("loop_back_mode_config: {:#?}", loop_back_mode_config);
        let data = ads1293
            .stream_one()
            .expect("failed to read data under stream mode");
        log::info!("data: {:#?}", data);

        led_pin.set_high().unwrap();
        FreeRtos::delay_ms(5000);
        led_pin.set_low().unwrap();
        FreeRtos::delay_ms(200);
    }
}
