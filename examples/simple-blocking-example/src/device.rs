use std::cell::RefCell;

use embedded_hal_bus::i2c::RefCellDevice;
use esp_idf_svc::hal::{
    delay::FreeRtos,
    i2c::{I2cConfig, I2cDriver},
    units::FromValueType,
};
use esp_idf_sys::{i2c_get_timeout, i2c_set_timeout};
use mlx9061x::{
    ic::{Mlx90614, Mlx90615},
    Mlx9061x, SlaveAddr,
};

pub fn setup_i2c(i2c: I2cDriver) -> RefCell<I2cDriver<'_>> {
    let i2c = {
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

        RefCell::new(i2c)
    };

    i2c
}

pub fn setup_i2c_devices<'a, 'b>(
    i2c: &'b RefCell<I2cDriver<'a>>,
) -> (
    bno055::Bno055<RefCellDevice<'b, I2cDriver<'a>>>,
    Mlx9061x<RefCellDevice<'b, I2cDriver<'a>>, Mlx90614>,
) {
    let bno055 = {
        let i2c = RefCellDevice::new(i2c);

        let mut bno055 = bno055::Bno055::new(i2c);

        bno055.init(&mut FreeRtos).unwrap();

        bno055
            .set_mode(bno055::BNO055OperationMode::NDOF, &mut FreeRtos)
            .unwrap();

        bno055
    };

    let mlx90614 = {
        let i2c = RefCellDevice::new(i2c);

        let address = SlaveAddr::Alternative(0x5a);

        let mut device = Mlx9061x::new_mlx90614(i2c, address, 5).unwrap();

        let id = device.device_id().unwrap();

        log::info!("Connected to MXL90614 sensor. ID: {}.", id);

        device
    };

    (bno055, mlx90614)
}
