use std::net::{SocketAddr, SocketAddrV4, UdpSocket};

use normal_data::{ServiceMessage, SERVICE_MESSAGE_SERIALIZE_MAX_LEN, SERVICE_NAME};

#[derive(Debug, thiserror::Error)]
pub enum ServiceDiscoveryError {
    #[error("the attempt to deserialize the data received from the service discovery port ({}) has reached its maximum limit.", .0)]
    DeserializationFailed(u16),
    #[error("the ipv6 address is not supported. the address is {}", .0)]
    Ipv6NotSupported(SocketAddr)
}

pub fn discover_service(port: u16) -> Result<SocketAddrV4, ServiceDiscoveryError> {
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
        return Err(ServiceDiscoveryError::Ipv6NotSupported(addr))
    };

    Ok(addr)
}