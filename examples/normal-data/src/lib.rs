use serde::{Deserialize, Serialize};

pub const DATA_SERIALIZE_MAX_LEN: usize = 256;
pub const PUSH_DATA_ENDPOINT_WS: &'static str = "/push-data";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Temperature {
    pub object1: f32,
    pub ambient: f32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Data {
    pub id: u32,
    // The first channel & second channel.
    pub ecg: (u32, u32),
    pub quaternion: mint::Quaternion<f32>,
    pub accel: mint::Vector3<f32>,
    pub temperature: Temperature,
}

impl Data {
    pub fn deserialize_from_json(slice: &[u8]) -> Result<Data, serde_json::Error> {
        serde_json::from_slice::<Data>(slice)
    }
}

pub const SERVICE_NAME: &'static str = "ADS1293-DEMO-NORMAL-SERVICE";

pub const SERVICE_MESSAGE_SERIALIZE_MAX_LEN: usize = 128;

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceMessage {
    pub service: String,
    pub bind_to: BindTo,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BindTo {
    pub ip: String,
    pub port: u16,
    pub udp_port: u16,
}

impl ServiceMessage {
    pub fn deserialize_from_json(slice: &[u8]) -> Result<ServiceMessage, serde_json::Error> {
        serde_json::from_slice::<ServiceMessage>(slice)
    }
}

#[cfg(test)]
mod tests {
    use mint::{Quaternion, Vector3};

    use crate::{
        Data, ServiceMessage, DATA_SERIALIZE_MAX_LEN, SERVICE_MESSAGE_SERIALIZE_MAX_LEN,
        SERVICE_NAME,
    };

    #[test]
    fn test_sufficient_serialize_max_len() {
        let json = serde_json::to_vec(&Data {
            id: u32::MAX,
            ecg: (u32::MAX, u32::MAX),
            quaternion: Quaternion::from([f32::MIN, f32::MIN, f32::MIN, f32::MIN]),
            accel: Vector3::from([f32::MAX, f32::MAX, f32::MAX]),
            temperature: crate::Temperature {
                object1: f32::MAX,
                ambient: f32::MAX,
            },
        })
        .unwrap();
        dbg!(json.len());
        dbg!(String::from_utf8(json.clone()));
        assert!(json.len() <= DATA_SERIALIZE_MAX_LEN);

        let service_info = ServiceMessage {
            service: SERVICE_NAME.to_string(),
            bind_to: crate::BindTo {
                ip: "192.168.100.100".to_owned(),
                port: 15303,
                udp_port: 15304,
            },
        };
        let json = serde_json::to_vec(&service_info).unwrap();
        dbg!(json.len());
        dbg!(String::from_utf8(json.clone()));
        assert!(json.len() <= SERVICE_MESSAGE_SERIALIZE_MAX_LEN);
    }
}
