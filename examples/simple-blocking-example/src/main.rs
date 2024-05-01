use std::{sync::mpsc, thread, time::Duration};

use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{
        delay::FreeRtos,
        gpio::PinDriver,
        i2c::{I2cConfig, I2cDriver},
        peripherals::Peripherals,
        spi::{self, SpiDeviceDriver, SpiDriver, SpiDriverConfig},
        units::{FromValueType, Hertz},
    },
    nvs::EspDefaultNvsPartition,
    timer::EspTaskTimerService,
    wifi::{BlockingWifi, EspWifi},
};
use esp_idf_sys::{i2c_get_timeout, i2c_set_timeout};
use normal_data::Data;
use simple_blocking_example::{
    data::{init_ads1293, retrieve_data_once},
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

    // let mut ws_client = create_ws_client(ws_socket_addr);

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

    let mut bno055 = {
        let sda = peripherals.pins.gpio21;
        let scl = peripherals.pins.gpio22;

        let i2c = {
            let baudrate = 100u32.kHz();
            let config = I2cConfig::new().baudrate(baudrate.into());

            let i2c = I2cDriver::new(peripherals.i2c0, sda, scl, &config).unwrap();

            #[allow(unused_labels)]
            'set_i2c_timeout: {
                let i2c_port = i2c.port();

                let mut timeout: std::os::raw::c_int = 0;

                unsafe { i2c_get_timeout(i2c_port, &mut timeout) };

                log::info!("Current i2c timeout: {}", timeout);

                timeout = 200_000;

                unsafe { i2c_set_timeout(i2c_port, timeout) };

                log::info!("Current i2c timeout is set to: {}", timeout);
            }

            i2c
        };

        let mut bno055 = bno055::Bno055::new(i2c);

        bno055.init(&mut FreeRtos).unwrap();

        bno055.set_mode(bno055::BNO055OperationMode::NDOF, &mut FreeRtos)
            .unwrap();

        bno055
    };

    let (data_tx, data_rx) = mpsc::sync_channel(1);

    let _transport_thread = thread::Builder::new()
        .stack_size(8192)
        .spawn(move || udp_transport_thread(udp_socket_addr, data_rx))
        .unwrap();

    let timer_service = EspTaskTimerService::new().unwrap();

    let timer = {
        let mut id = 0;
        timer_service.timer(move || {
            let ecg = retrieve_data_once(&mut ads1293);
            let quaternion = bno055.quaternion().unwrap();

            id += 1;
            let data = Data { id, ecg, quaternion };
            log::debug!("{:?}", data);

            data_tx.send(data).unwrap();
        })
    }
    .unwrap();
    timer.every(Duration::from_millis(20)).unwrap();

    let mut is_pin_high = true;
    loop {
        in_program_blink(&mut led_pin, &mut is_pin_high);
        FreeRtos::delay_ms(1000);
        log::debug!("wifi: {:?}", wifi.is_up().unwrap())
    }
}
