use embedded_hal::spi::SpiDevice;

use crate::driver::initialization::{Application3Lead, InitializeError, Initializer};
use crate::driver::registers::access::{ReadError, ReadFromRegister, WriteToRegister};
use crate::driver::registers::CONFIG;
use crate::driver::registers::data::MainConfig;

use self::operator::Operator;

pub mod initialization;
pub mod operator;
pub mod registers;

pub struct ADS1293<SPI: SpiDevice> {
    operator: Operator<SPI>,
}

impl<SPI: SpiDevice> ADS1293<SPI> {
    pub fn new(spi: SPI) -> ADS1293<SPI> {
        ADS1293 {
            operator: Operator::new(spi),
        }
    }
}

impl<SPI: SpiDevice> Initializer<Application3Lead> for ADS1293<SPI> {
    type SpiError = SPI::Error;

    fn init(
        &mut self,
        _application: Application3Lead,
    ) -> Result<(), InitializeError<Self::SpiError>> {
        struct AddressData(u8, u8);

        const INITIAL_ADDRESS_DATA_ARR: &'static [AddressData] = &[
            AddressData(0x01, 0x11),
            AddressData(0x02, 0x19),
            AddressData(0x0A, 0x07),
            AddressData(0x0C, 0x04),
            AddressData(0x12, 0x04),
            AddressData(0x14, 0x24),
            AddressData(0x21, 0x02),
            AddressData(0x22, 0x02),
            AddressData(0x23, 0x02),
            AddressData(0x27, 0x08),
            AddressData(0x2F, 0x30),
            AddressData(0x00, 0x01),
        ];

        for address_data in INITIAL_ADDRESS_DATA_ARR {
            self.operator
                .write(address_data.0, address_data.1)
                .map_err(|e| InitializeError::WriteError {
                    source: e,
                    address: address_data.0,
                    data: address_data.1,
                })?;
        }

        Ok(())
    }
}

impl<SPI: SpiDevice> ReadFromRegister<registers::CONFIG, MainConfig, SPI::Error> for ADS1293<SPI> {
    fn read(&mut self, register: CONFIG) -> Result<MainConfig, ReadError<SPI::Error>> {
        
        todo!()
    }
}
