use anyhow::Context;
use futures::io::ReuniteError;
use network_interface::{NetworkInterface, NetworkInterfaceConfig};
use normal_data::BindTo;
use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
pub struct BroadcastInfo {
    pub ip: String,
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Database {
    pub url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub bind_to: BindTo,
    pub broadcast: BroadcastInfo,
    pub database: Database,
}

impl Settings {
    pub fn new() -> anyhow::Result<Settings> {
        use config::{Config, File, FileFormat};

        let default_settings = include_str!("presets/default_settings.toml");

        let dev_local_settings_path = "./_dev_things/settings.toml";

        let local_settings_path = "./settings.toml";

        let mut config: Settings = Config::builder()
            .add_source(File::from_str(default_settings, FileFormat::Toml))
            .add_source(File::with_name(dev_local_settings_path).required(false))
            .add_source(File::with_name(&local_settings_path).required(false))
            .build()?
            .try_deserialize()?;

        if config.broadcast.ip == "255.255.255.255" {
            log::info!("The broadcast IP is not set. Begin to find a most relevant IP address.");
            config
                .replace_broadcast()
                .context("Encounter error when replacing the broadcast address.")?;
            log::info!("Found a broadcast IP: {}", config.broadcast.ip)
        }

        Ok(config)
    }

    fn replace_broadcast(&mut self) -> anyhow::Result<()> {
        let network_interfaces = NetworkInterface::show().unwrap();
        let wlan = network_interfaces
            .into_iter()
            .filter(|interface| interface.name == "WLAN")
            .collect::<Vec<_>>();

        if wlan.len() == 0 {
            return Err(anyhow::anyhow!("cannot find WLAN interface"));
        } else if wlan.len() > 1 {
            unreachable!("There are more than one network interfaces with name 'WLAN'!")
        }

        let wlan = &wlan[0];

        let v4_addr = wlan.addr.get(1).unwrap();

        let broadcast_addr = v4_addr.broadcast().unwrap();

        self.broadcast.ip = broadcast_addr.to_string();

        Ok(())
    }
}

#[cfg(test)]
mod debug {
    use network_interface::{NetworkInterface, NetworkInterfaceConfig};

    #[ignore]
    #[test]
    fn network_interfaces() {
        env_logger::init();
        let network_interfaces = NetworkInterface::show().unwrap();
        for interface in network_interfaces {
            log::info!("{:?}", (interface.name, interface.addr[1].broadcast()));
        }
    }
}
