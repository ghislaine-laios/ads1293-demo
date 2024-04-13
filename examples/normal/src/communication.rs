use crate::settings::Settings;
use anyhow::Context;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::modem::WifiModemPeripheral;
use esp_idf_svc::hal::peripheral::Peripheral;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::timer::EspTaskTimerService;
use esp_idf_svc::wifi;
use esp_idf_svc::wifi::AsyncWifi;
use esp_idf_svc::wifi::ClientConfiguration;
use esp_idf_svc::wifi::EspWifi;
use esp_idf_sys::esp;
use futures_util::SinkExt;
use normal_data::Data;
use normal_data::ServiceMessage;
use normal_data::PUSH_DATA_ENDPOINT;
use normal_data::SERVICE_MESSAGE_SERIALIZE_MAX_LEN;
use normal_data::SERVICE_NAME;
use std::net::SocketAddr;
use std::net::SocketAddrV4;
use std::net::UdpSocket;
use tokio::net::TcpStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use esp_idf_sys::esp_wifi_set_ps;
use esp_idf_sys::wifi_ps_type_t_WIFI_PS_NONE;

pub struct ConnectWifiPayload<M: WifiModemPeripheral, Modem: Peripheral<P = M>> {
    pub modem: Modem,
    pub sys_loop: EspSystemEventLoop,
    pub nvs: EspDefaultNvsPartition,
    pub timer_service: EspTaskTimerService,
}

pub async fn communication(
    socket: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    mut data_receiver: tokio::sync::mpsc::Receiver<Data>,
) {
    let mut count = 0;
    while let Some(data) = data_receiver.recv().await {
        // log::debug!("{:?}", data);
        let str = serde_json::to_string(&data)
            .map_err(|e| format!("cannot serialize the data: {:#?}", e))
            .unwrap();
        socket.feed(Message::text(str)).await.unwrap();

        count += 1;
        if count == 3 {
            log::debug!("flush! last data id: {}", data.id);
            socket.flush().await.unwrap();
            log::debug!("flushed!");
            count = 0;
        }
    }
}

pub async fn connect_wifi<'d, M: WifiModemPeripheral>(
    settings: &Settings,
    modem: impl Peripheral<P = M> + 'd,
    sys_loop: EspSystemEventLoop,
    nvs: EspDefaultNvsPartition,
    timer_service: EspTaskTimerService,
) -> anyhow::Result<AsyncWifi<EspWifi<'d>>> {
    let wifi_config = wifi::Configuration::Client(ClientConfiguration {
        ssid: settings.wifi.ssid.clone(),
        bssid: None,
        auth_method: wifi::AuthMethod::WPA2Personal,
        password: settings.wifi.password.clone(),
        channel: None,
        ..Default::default()
    });

    let mut wifi = EspWifi::new(modem, sys_loop.clone(), Some(nvs))
        .expect("failed to create esp-wifi service");

    wifi.driver_mut().set_rssi_threshold(-40).unwrap();

    let mut wifi = AsyncWifi::wrap(wifi, sys_loop, timer_service)
        .expect("failed to create async wifi service");

    wifi.set_configuration(&wifi_config)
        .expect("failed to set wifi configuration");

    wifi.start().await.expect("failed to start the wifi");

    wifi.connect().await.context("failed to connect wifi")?;

    wifi.wait_netif_up()
        .await
        .expect("failed to call wait_netif_up on wifi service");
    
    esp!(unsafe {esp_wifi_set_ps(wifi_ps_type_t_WIFI_PS_NONE)}).unwrap();

    Ok(wifi)
}

pub async fn setup_websocket(
    addr: SocketAddrV4,
) -> WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>> {
    let url = format!("ws://{}{}", addr, PUSH_DATA_ENDPOINT);

    log::info!("websocket is connecting to {}", url.as_str());

    let (socket, resp) = connect_async(url).await.unwrap();

    log::info!("websocket resp: {:?}", resp);

    socket
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceDiscoveryError {
    #[error("the attempt to deserialize the data received from the service discovery port ({}) has reached its maximum limit.", .0)]
    DeserializationFailed(u16),
}

pub fn discover_service(port: u16) -> Result<(SocketAddr, u16), ServiceDiscoveryError> {
    log::debug!("Starting to discover the service");
    let socket = UdpSocket::bind(("0.0.0.0", port)).expect("failed to bind the udp socket");
    let mut buf = [0; SERVICE_MESSAGE_SERIALIZE_MAX_LEN];
    let mut deserialize_attempts_count: usize = 0;

    let (service_info, addr) = loop {
        if deserialize_attempts_count >= 30 {
            return Err(ServiceDiscoveryError::DeserializationFailed(port));
        }

        let (read_size, addr) = socket
            .recv_from(&mut buf)
            .expect("failed to recv data from the udp socket");

        let service_info = match ServiceMessage::deserialize_from_json(&buf[..read_size]) {
            Ok(m) => m,
            Err(e) => {
                deserialize_attempts_count += 1;
                log::warn!("Can't deserialize the received message from the service discovery udp socket. Error: {:#?}", e);
                continue;
            }
        };

        if service_info.service.as_str() == SERVICE_NAME {
            break (service_info, addr);
        };

        log::debug!("receive service info: {:#?}", service_info);
    };

    Ok((addr, service_info.bind_to.port))
}
