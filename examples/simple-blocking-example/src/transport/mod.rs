use esp_idf_svc::ws::client::{EspWebSocketClient, EspWebSocketClientConfig};
use normal_data::{
    ServiceMessage, PUSH_DATA_ENDPOINT_WS, SERVICE_MESSAGE_SERIALIZE_MAX_LEN, SERVICE_NAME,
};
use std::{
    net::{SocketAddr, SocketAddrV4, UdpSocket},
    time::Duration,
};

pub mod udp;

#[derive(Debug, thiserror::Error)]
pub enum ServiceDiscoveryError {
    #[error("the attempt to deserialize the data received from the service discovery port ({}) has reached its maximum limit.", .0)]
    DeserializationFailed(u16),
    #[error("the ipv6 address is not supported. the address is {}", .0)]
    Ipv6NotSupported(SocketAddr),
}

pub fn discover_service(port: u16) -> Result<(SocketAddrV4, SocketAddrV4), ServiceDiscoveryError> {
    log::info!("Starting to discover the service on port {}", port);
    let socket = UdpSocket::bind(("0.0.0.0", port)).expect("failed to bind the udp socket");
    let mut buf = [0; SERVICE_MESSAGE_SERIALIZE_MAX_LEN];
    let mut deserialize_attempts_count: usize = 0;

    let (service_info, mut addr) = loop {
        if deserialize_attempts_count >= 30 {
            return Err(ServiceDiscoveryError::DeserializationFailed(port));
        }

        let (read_size, addr) = socket
            .recv_from(&mut buf)
            .expect("failed to recv data from the udp socket");

        log::debug!("{:?}", buf);

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

    addr.set_port(service_info.bind_to.port);

    let SocketAddr::V4(addr) = addr else {
        return Err(ServiceDiscoveryError::Ipv6NotSupported(addr));
    };

    let mut udp_addr = addr.clone();
    udp_addr.set_port(service_info.bind_to.udp_port);

    Ok((addr, udp_addr))
}

pub fn create_ws_client(server_socket_addr: SocketAddrV4) -> EspWebSocketClient<'static> {
    let url = format!("ws://{}{}", server_socket_addr, PUSH_DATA_ENDPOINT_WS);

    log::info!("Setting up the websocket client to connect to {}", url);

    EspWebSocketClient::new(
        &url,
        &EspWebSocketClientConfig {
            network_timeout_ms: Duration::from_millis(500),
            reconnect_timeout_ms: Duration::from_millis(10),
            ..Default::default()
        },
        Duration::from_millis(30),
        |_| {},
    )
    .unwrap()
}
