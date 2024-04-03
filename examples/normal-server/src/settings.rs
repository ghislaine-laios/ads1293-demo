use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BindTo {
    pub ip: String,
    pub port: u16,
}

#[derive(Deserialize, Clone, Debug)]
pub struct BroadcastInfo {
    pub ip: String,
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub bind_to: BindTo,
    pub broadcast: BroadcastInfo,
}

impl Settings {
    pub fn new() -> Result<Settings, config::ConfigError> {
        use config::{Config, File, FileFormat};

        let default_settings = include_str!("presets/default_settings.toml");

        let local_settings_path = "./_dev_things/settings.toml";

        let config = Config::builder()
            .add_source(File::from_str(default_settings, FileFormat::Toml))
            .add_source(File::with_name(local_settings_path).required(false))
            .build()?
            .try_deserialize()?;

        Ok(config)
    }
}