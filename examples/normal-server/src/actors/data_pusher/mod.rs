use super::{
    data_hub::LaunchedDataHub,
    websocket::actor_context::WebsocketActorContext,
};
use crate::actors::data_pusher::id::NEXT_DATA_PUSHER_ID;
use std::time::Duration;

pub mod actions;
mod handler;
mod id;
mod launched;

use actix_web::web::{Bytes, Payload};
use futures::{future::pending, Stream};
pub use id::DataPusherId;
pub use launched::LaunchedDataPusher;
use tokio::sync::mpsc;

pub struct DataPusherBeforeLaunched {}

#[derive(Debug)]
pub struct DataPusher {
    id: DataPusherId,
    data_hub: LaunchedDataHub,
    launched_self: LaunchedDataPusher,
}

impl DataPusher {
    pub async fn launch_inline(
        payload: Payload,
        launched_data_hub: LaunchedDataHub,
    ) -> impl Stream<Item = Result<Bytes, super::websocket::actor_context::TaskExecutionError>> {
        let (action_tx, action_rx) = mpsc::channel(1);

        WebsocketActorContext::launch_inline(
            payload,
            action_rx,
            Self {
                id: {
                    let mut guard = NEXT_DATA_PUSHER_ID.lock().await;
                    *guard += 1;
                    *guard
                },
                data_hub: launched_data_hub,
                launched_self: LaunchedDataPusher { tx: action_tx },
            },
            Duration::from_secs(5),
            Duration::from_secs(1),
            pending(),
        )
    }
}
