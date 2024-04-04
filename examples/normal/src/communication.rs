use crate::settings::Settings;
use crate::settings::SETTINGS;
use ads1293_demo::driver::registers::data;
use anyhow::Context;
use embassy_futures::select;
use embassy_futures::select::Either;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Receiver;
use embassy_sync::channel::TrySendError;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::modem::WifiModemPeripheral;
use esp_idf_svc::hal::peripheral::Peripheral;
use esp_idf_svc::io::EspIOError;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::timer::EspTaskTimerService;
use esp_idf_svc::wifi;
use esp_idf_svc::wifi::AsyncWifi;
use esp_idf_svc::wifi::ClientConfiguration;
use esp_idf_svc::wifi::EspWifi;
use esp_idf_svc::ws::client::EspWebSocketClient;
use esp_idf_svc::ws::client::EspWebSocketClientConfig;
use esp_idf_svc::ws::FrameType;
use esp_idf_sys::EspError;
use normal_data::Data;
use normal_data::ServiceMessage;
use normal_data::PUSH_DATA_ENDPOINT;
use normal_data::SERVICE_MESSAGE_SERIALIZE_MAX_LEN;
use normal_data::SERVICE_NAME;
use std::net::SocketAddr;
use std::net::UdpSocket;
use std::time::Duration;

pub struct ConnectWifiPayload<M: WifiModemPeripheral, Modem: Peripheral<P = M>> {
    pub modem: Modem,
    pub sys_loop: EspSystemEventLoop,
    pub nvs: EspDefaultNvsPartition,
    pub timer_service: EspTaskTimerService,
}

pub async fn communication<'d, M: WifiModemPeripheral, Modem: Peripheral<P = M>>(
    connect_wifi_payload: ConnectWifiPayload<M, Modem>,
    data_receiver: Receiver<'d, CriticalSectionRawMutex, Data, 256>,
) -> anyhow::Result<()> {
    let settings = SETTINGS.get().expect("the settings are not initialized");

    let _wifi = connect_wifi(
        settings,
        connect_wifi_payload.modem,
        connect_wifi_payload.sys_loop,
        connect_wifi_payload.nvs,
        connect_wifi_payload.timer_service,
    )
    .await?;

    loop {
        if let Err(e) = serve(settings.service.broadcast_port, data_receiver).await {
            log::warn!("failed to serve: {:?}", e);
        }
    }

    Ok(())
}

async fn connect_wifi<'d, M: WifiModemPeripheral>(
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

    let mut wifi = AsyncWifi::wrap(
        EspWifi::new(modem, sys_loop.clone(), Some(nvs))
            .expect("failed to create esp-wifi service"),
        sys_loop,
        timer_service,
    )
    .expect("failed to create async wifi service");

    wifi.set_configuration(&wifi_config)
        .expect("failed to set wifi configuration");

    wifi.start().await.expect("failed to start the wifi");

    wifi.connect().await.context("failed to connect wifi")?;

    wifi.wait_netif_up()
        .await
        .expect("failed to call wait_netif_up on wifi service");

    Ok(wifi)
}

#[derive(Debug, thiserror::Error)]
enum ServiceDiscoveryError {
    #[error("the attempt to deserialize the data received from the service discovery port ({}) has reached its maximum limit.", .0)]
    DeserializationFailed(u16),
}

fn discover_service(port: u16) -> Result<(SocketAddr, u16), ServiceDiscoveryError> {
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

#[derive(Debug, thiserror::Error)]
enum ServeError {
    #[error("failed to establish websocket connection with the sever")]
    ConnectFailed(#[source] EspIOError),
    #[error("failed to send data through websocket")]
    SendFailed(#[source] EspError),
}

static WS_CHANNEL: embassy_sync::channel::Channel<
    CriticalSectionRawMutex,
    (FrameType, Vec<u8>),
    4,
> = embassy_sync::channel::Channel::new();

async fn serve<'d>(
    broadcast_port: u16,
    data_receiver: Receiver<'d, CriticalSectionRawMutex, Data, 256>,
) -> Result<(), ServeError> {
    let (addr, port) = discover_service(broadcast_port).expect("failed to discover the service");

    let SocketAddr::V4(mut addr) = addr else {
        panic!("the service use ipv6 stack but it's not supported");
    };

    addr.set_port(port);
    let url = format!("ws://{}{}", addr, PUSH_DATA_ENDPOINT);

    let ws_sender = WS_CHANNEL.sender();
    let ws_receiver = WS_CHANNEL.receiver();

    let mut ws_client = EspWebSocketClient::new(
        &url,
        &EspWebSocketClientConfig {
            ping_interval_sec: Duration::from_secs(1),
            reconnect_timeout_ms: Duration::from_millis(200),
            network_timeout_ms: Duration::from_millis(200),
            ..Default::default()
        },
        Duration::from_secs(10),
        move |event| {
            let event = match event {
                Ok(e) => e,
                Err(e) => {
                    log::error!("received error websocket event: {:?}", e);
                    return;
                }
            };

            match event.event_type {
                // esp_idf_svc::ws::client::WebSocketEventType::BeforeConnect => todo!(),
                // esp_idf_svc::ws::client::WebSocketEventType::Connected => todo!(),
                // esp_idf_svc::ws::client::WebSocketEventType::Disconnected => todo!(),
                // esp_idf_svc::ws::client::WebSocketEventType::Close(_) => todo!(),
                // esp_idf_svc::ws::client::WebSocketEventType::Closed => todo!(),
                // esp_idf_svc::ws::client::WebSocketEventType::Text(_) => todo!(),
                // esp_idf_svc::ws::client::WebSocketEventType::Binary(_) => todo!(),
                // esp_idf_svc::ws::client::WebSocketEventType::Ping => {
                //     ws_sender.send(FrameType::Pong);
                // }
                esp_idf_svc::ws::client::WebSocketEventType::Pong => {
                    log::debug!("websocket pong.");
                }
                _ => {}
            };
        },
    )
    .map_err(ServeError::ConnectFailed)?;

    let mut last_data = None;
    let mut counter = 0;
    loop {
        let result = select::select(
            async {
                let msg = ws_receiver.receive().await;
                counter += 1;
                if counter >= 60 {
                    log::info!("msg: {:?}", msg);
                    counter = 0;
                }
                // ws_client
                //     .send(msg.0, msg.1.as_ref())
                //     .map_err(ServeError::SendFailed)?;

                Ok::<(), ServeError>(())
            },
            async {
                // For cancellation safety
                if let Some(data) = &last_data {
                    let raw = serde_json::to_vec(data).expect("failed to serialize the data");
                    if let Err(TrySendError::Full(r)) = ws_sender.try_send((FrameType::Text(false), raw)) {
                        log::warn!("the ws_channel is full. unexpected.");
                        ws_sender.send(r).await;
                    }
                    last_data = None;
                }
                last_data = Some(data_receiver.receive().await);
            },
        )
        .await;

        if let Either::First(result) = result {
            result?;
        }

        // let water_mark =
        //     unsafe { esp_idf_sys::uxTaskGetStackHighWaterMark2(core::ptr::null_mut()) };
        // dbg!(water_mark);
    }

    Ok(())
}
