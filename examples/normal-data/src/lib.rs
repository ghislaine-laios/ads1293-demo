use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct Data {
    pub id: u32,
    pub value: u32,
}

pub const DATA_SERIALIZE_MAX_LEN: usize = 32;

impl Data {
    pub fn deserialize_from_json(slice: &[u8]) -> Result<Data, serde_json::Error> {
        serde_json::from_slice::<Data>(slice)
    }
}

pub const SERVICE_NAME: &'static str = "ADS1293-DEMO-NORMAL-SERVICE";

pub const SERVICE_MESSAGE_SERIALIZE_MAX_LEN: usize = 96;

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceMessage {
    pub service: String,
    pub bind_to: BindTo,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BindTo {
    pub ip: String,
    pub port: u16,
}

impl ServiceMessage {
    pub fn deserialize_from_json(slice: &[u8]) -> Result<ServiceMessage, serde_json::Error> {
        serde_json::from_slice::<ServiceMessage>(slice)
    }
} 

#[cfg(test)]
mod tests {
    use crate::{Data, ServiceMessage, DATA_SERIALIZE_MAX_LEN, SERVICE_MESSAGE_SERIALIZE_MAX_LEN, SERVICE_NAME};

    #[test]
    fn test_sufficient_serialize_max_len() {
        let json = serde_json::to_vec(&Data { id: 1, value: 10 }).unwrap();
        dbg!(json.len());
        assert!(json.len() <= DATA_SERIALIZE_MAX_LEN);

        let service_info = ServiceMessage {
            service: SERVICE_NAME.to_string(),
            bind_to: crate::BindTo {
                ip: "192.168.100.100".to_owned(),
                port: 15303,
            },
        };
        let json = serde_json::to_vec(&service_info).unwrap();
        dbg!(json.len());
        assert!(json.len() <= SERVICE_MESSAGE_SERIALIZE_MAX_LEN);
    }
}
