use ads1293_demo::driver::{
    initialization::{InitializeError, Initializer},
    registers::access::WriteToRegister,
    ADS1293,
};
use embedded_hal::spi::SpiDevice;

pub struct ApplicationModified3Lead;

impl<SPI: SpiDevice> Initializer<ApplicationModified3Lead> for ADS1293<SPI> {
    type SpiError = SPI::Error;

    fn init(
        &mut self,
        _application: ApplicationModified3Lead,
    ) -> Result<(), ads1293_demo::driver::initialization::InitializeError<Self::SpiError>> {
        struct AddressData(u8, u8);

        const INITIAL_ADDRESS_DATA_ARR: &'static [AddressData] = &[
            // CH1, INP -> IN2, INN -> IN3
            // TST1 -> 00, POS1 -> 010, NEG1 -> 011
            AddressData(0x01, 0x13),
            // CH2, INP -> IN5, INN -> IN3,
            // TST2 -> 00, POS2 -> 101, NEG2 -> 011
            AddressData(0x02, 0x2B),
            // Common mode detection for IN1, IN2 and IN3
            AddressData(0x0A, 0x07),
            AddressData(0x0C, 0x04),
            AddressData(0x12, 0x04),
            AddressData(0x14, 0x24),
            AddressData(0x21, 0x02),
            AddressData(0x22, 0x02),
            AddressData(0x23, 0x02),
            AddressData(0x27, 0x08),
            AddressData(0x2F, 0x31),
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
