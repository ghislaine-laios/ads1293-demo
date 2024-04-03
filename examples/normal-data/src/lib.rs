use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct Data {
    pub id: u32,
    pub value: u32,
}

pub const DATA_SERIALIZE_MAX_LEN: usize = 64;

#[cfg(test)]
mod tests {
    #[test]
    fn test_sufficient_serialize_max_len() {
        let buf = 
    }
}