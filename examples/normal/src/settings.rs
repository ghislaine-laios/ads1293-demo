use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Wifi {
    ssid: String,
    password: String
}

#[derive(Debug, Deserialize)]
struct Settings {
    wifi: Wifi
}