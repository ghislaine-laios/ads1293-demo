use std::{
    net::{SocketAddrV4, UdpSocket},
    sync::mpsc,
    time::Duration,
};

use ads1293_demo::driver::registers::data;
use normal_data::Data;

pub fn udp_transport_thread(udp_server_addr: SocketAddrV4, rx: mpsc::Receiver<Data>) {
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    socket.connect(udp_server_addr).unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs_f64(1.0 / 60.0)))
        .unwrap();

    loop {
        let data = rx.recv().unwrap();
        let str = serde_json::to_vec(&data).unwrap();
        if let Err(e) = socket.send(&str) {
            log::warn!("failed to send data. error_msg: {}", e)
        }
    }
}
