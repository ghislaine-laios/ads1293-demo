use serde::{Deserialize, Serialize};

pub const DATA_SERIALIZE_MAX_LEN: usize = 128;
pub const PUSH_DATA_ENDPOINT_WS: &'static str = "/push-data";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Data {
    pub id: u32,
    pub ecg: u32,
    pub quaternion: mint::Quaternion<f32>,
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
    use mint::Quaternion;

    use crate::{
        Data, ServiceMessage, DATA_SERIALIZE_MAX_LEN, SERVICE_MESSAGE_SERIALIZE_MAX_LEN,
        SERVICE_NAME,
    };

    #[test]
    fn test_sufficient_serialize_max_len() {
        let json = serde_json::to_vec(&Data {
            id: 1,
            ecg: 10,
            quaternion: Quaternion::from([f32::MIN, f32::MIN, f32::MIN, f32::MIN]),
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
