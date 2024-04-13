use esp_idf_svc::wifi::{self, BlockingWifi, ClientConfiguration, EspWifi};

use crate::settings::Settings;

pub fn connect_wifi(settings: &Settings, wifi: &mut BlockingWifi<EspWifi<'static>>) {
    let wifi_config = wifi::Configuration::Client(ClientConfiguration {
        ssid: settings.wifi.ssid.clone(),
        bssid: None,
        auth_method: wifi::AuthMethod::WPA2Personal,
        password: settings.wifi.password.clone(),
        channel: None,
        ..Default::default()
    });

    wifi.set_configuration(&wifi_config).unwrap();

    wifi.start().unwrap();
    log::info!("Wifi started.");

    wifi.connect().unwrap();
    log::info!("Wifi connected.");

    wifi.wait_netif_up().unwrap();
    log::info!("Wifi netif up.");
}