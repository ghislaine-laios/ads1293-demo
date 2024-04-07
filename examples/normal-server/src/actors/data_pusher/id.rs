use async_std::sync::Mutex;

pub type DataPusherId = u32;

pub(super) static NEXT_DATA_PUSHER_ID: Mutex<DataPusherId> = Mutex::new(0);
