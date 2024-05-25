use std::{
    cell::{OnceCell, RefCell},
    pin::pin,
    sync::mpsc,
    thread,
    time::Duration,
};

use bno055::mint::Quaternion;
use embedded_hal_bus::i2c::RefCellDevice;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{
        delay::FreeRtos,
        gpio::PinDriver,
        i2c::{I2cConfig, I2cDriver},
        peripherals::Peripherals,
        spi::{self, SpiDeviceDriver, SpiDriver, SpiDriverConfig},
        task,
        units::{FromValueType, Hertz},
    },
    nvs::EspDefaultNvsPartition,
    timer::EspTaskTimerService,
    wifi::{BlockingWifi, EspWifi},
};
use esp_idf_sys::{i2c_get_timeout, i2c_set_timeout};
use normal_data::{Data, Temperature};
use simple_blocking_example::{
    data::{init_ads1293, retrieve_data_two_channel},
    device::{setup_i2c, setup_i2c_devices},
    led::{in_program_blink, start_program_blink},
    settings::Settings,
    transport::{discover_service, udp::udp_transport_thread},
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

    let (_ws_socket_addr, udp_socket_addr) =
        discover_service(settings.service.broadcast_port).unwrap();

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
    };

    let (data_tx, data_rx) = mpsc::sync_channel(1);

    let _transport_thread = thread::Builder::new()
        .stack_size(8192)
        .spawn(move || udp_transport_thread(udp_socket_addr, data_rx))
        .unwrap();

    let timer_service = EspTaskTimerService::new().unwrap();

    let mut timer = timer_service.timer_async().unwrap();

    let mut is_pin_high = true;
    thread::spawn(move || loop {
        in_program_blink(&mut led_pin, &mut is_pin_high);
        FreeRtos::delay_ms(1000);
        log::debug!("wifi: {:?}", wifi.is_up().unwrap())
    });

    task::block_on(pin!(async move {
        let sda = peripherals.pins.gpio21;
        let scl = peripherals.pins.gpio22;

        let baudrate = 100u32.kHz();
        let config = I2cConfig::new().baudrate(baudrate.into());

        let i2c = I2cDriver::new(peripherals.i2c0, sda, scl, &config).unwrap();

        let i2c = setup_i2c(i2c);

        let mut id = 0;
        let mut devices = if settings.only_ecg {
            None
        } else {
            Some(setup_i2c_devices(&i2c))
        };

        loop {
            timer.after(Duration::from_millis(20)).await.unwrap();

            let (ecg1, ecg2) = retrieve_data_two_channel(&mut ads1293);

            let (quaternion, accel, obj1_temp, ambient_temp) = if let Some(devices) = &mut devices {
                let quaternion = devices.0.quaternion().unwrap();
                let accel = devices.0.linear_acceleration().unwrap();

                let obj1_temp = devices.1.object1_temperature().unwrap();
                let ambient_temp = devices.1.ambient_temperature().unwrap();

                (quaternion, accel, obj1_temp, ambient_temp)
            } else {
                (
                    [0f32, 0f32, 0f32, 0f32].into(),
                    [0f32, 0f32, 0f32].into(),
                    0f32,
                    0f32,
                )
            };

            id += 1;
            let data = Data {
                id,
                ecg: (ecg1, ecg2),
                quaternion,
                accel,
                temperature: Temperature {
                    object1: obj1_temp,
                    ambient: ambient_temp,
                },
            };
            log::debug!("{:?}", data);

            data_tx.send(data).unwrap();
        }
    }));
}
