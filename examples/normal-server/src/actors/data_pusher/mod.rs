use super::{
    data_hub::{registration_keepers::DataPusherRegistrationKeeper, LaunchedDataHubController},
    websocket::actor_context::WebsocketActorContext,
};
use crate::actors::data_pusher::id::NEXT_DATA_PUSHER_ID;
use std::time::Duration;

pub mod actions;
mod handler;
mod id;
mod launched;

use actix_web::web::{Bytes, Payload};
use anyhow::Context;
use futures::{future::pending, Stream};
pub use id::DataPusherId;
pub use launched::LaunchedDataPusher;
use tokio::sync::mpsc;

pub struct DataPusherBeforeLaunched {}

#[derive(Debug)]
pub struct DataPusher {
    id: DataPusherId,
    #[allow(unused)]
    registration_keeper: DataPusherRegistrationKeeper,
}

impl DataPusher {
    pub async fn launch_inline(
        payload: Payload,
        data_hub_controller: LaunchedDataHubController,
    ) -> anyhow::Result<
        impl Stream<Item = Result<Bytes, super::websocket::actor_context::TaskExecutionError>>,
    > {
        const FAILED_TO_CREATE_REGISTRATION_KEEPER_STR: &'static str =
            "failed to create the data pusher registration keeper when launching a data pusher inline";

        let (action_tx, action_rx) = mpsc::channel(1);

        let id = {
            let mut guard = NEXT_DATA_PUSHER_ID.lock().await;
            *guard += 1;
            *guard
        };

        anyhow::Ok(WebsocketActorContext::launch_inline(
            payload,
            action_rx,
            Self {
                id,
                registration_keeper: DataPusherRegistrationKeeper::new(
                    id,
                    data_hub_controller,
                    LaunchedDataPusher { tx: action_tx },
                )
                .context(FAILED_TO_CREATE_REGISTRATION_KEEPER_STR)?,
            },
            Duration::from_secs(5),
            Duration::from_secs(1),
            pending(),
        ))
    }
}
