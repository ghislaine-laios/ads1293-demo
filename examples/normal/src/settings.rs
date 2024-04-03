use once_cell::sync::OnceCell;
use serde::Deserialize;

pub static SETTINGS: OnceCell<Settings> = OnceCell::new();

#[derive(Debug, Deserialize)]
pub struct Wifi {
    pub ssid: heapless::String<32>,
    pub password: heapless::String<64>,
}

#[derive(Debug, Deserialize)]
pub struct Settings {
    pub wifi: Wifi,
}

impl Settings {
    pub fn init() -> Result<(), config::ConfigError> {
        use config::{Config, File, FileFormat};

        let default_settings = include_str!("../settings.default.toml");
        let settings = include_str!("../settings.toml");

        let config = Config::builder()
            .add_source(File::from_str(default_settings, FileFormat::Toml))
            .add_source(File::from_str(settings, FileFormat::Toml))
            .build()?
            .try_deserialize()?;

        SETTINGS.set(config).unwrap();

        Ok(())
    }
}
