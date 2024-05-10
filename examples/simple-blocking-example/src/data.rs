use ads1293_demo::driver::{
    initialization::Initializer,
    registers::{self, access::ReadFromRegister, DataRegister},
    ADS1293,
};
use embedded_hal::spi::SpiDevice;

use crate::application_modified_3_lead::ApplicationModified3Lead;

pub fn init_ads1293<SPI: SpiDevice>(spi: SPI) -> ADS1293<SPI> {
    let mut ads1293 = ADS1293::new(spi);

    ads1293
        .init(ApplicationModified3Lead)
        .expect("failed to init the ads1293");

    log::info!("ADS1293 initialized.");

    let main_config = ads1293
        .read(registers::CONFIG)
        .expect("failed to read the main config");

    log::info!("main_config: {:?}", main_config);

    let loop_back_mode_config = ads1293
        .read(registers::CH_CNFG)
        .expect("fail to read loop back mode config");

    log::info!("loop_back_mode_config: {:?}", loop_back_mode_config);

    ads1293
}

pub fn retrieve_data_once(ads1293: &mut ADS1293<impl SpiDevice>) -> u32 {
    let data_vec = ads1293
        .stream_one()
        .expect("failed to read data under stream mode");

    for data in data_vec {
        let DataRegister::DATA_CH1_ECG(data) = data else {
            continue;
        };

        return data.into();
    }

    unreachable!()
}

pub fn retrieve_data_two_channel(ads1293: &mut ADS1293<impl SpiDevice>) -> (u32, u32) {
    let data_vec = ads1293
        .stream_one()
        .expect("failed to read data under stream mode");

    let mut result = (0u32, 0u32);

    for data in data_vec {
        match data {
            DataRegister::DATA_CH1_ECG(ch1) => result.0 = ch1.into(),
            DataRegister::DATA_CH2_ECG(ch2) => result.1 = ch2.into(),
            _ => continue,
        }
    }

    result
}
