use self::actions::{Action, Close, NewData};

use super::{
    data_hub::{
        actions::{RegisterDataPusher, UnRegisterDataPusher},
        LaunchedDataHub,
    },
    handler::ContextHandler,
    websocket::{
        context::WebsocketContext,
        processor::{
            actions::{Started, Stopping},
            new_ws_processor, ProcessorBeforeLaunched, ProcessorMeta,
        },
        subtask::NoSubtask,
    },
    Handler,
};
use crate::actors::data_pusher::id::NEXT_DATA_PUSHER_ID;

pub mod actions;
mod id;
mod launched;

use actix_http::ws;
use actix_web::web::{Bytes, Payload};
use anyhow::Context;
use futures::Stream;
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

pub struct WsDataPusherWrapper(ProcessorBeforeLaunched, DataPusherId, LaunchedDataHub);

impl DataPusher {
    pub async fn new_ws_data_pusher(
        payload: Payload,
        launched_data_hub: LaunchedDataHub,
    ) -> WsDataPusherWrapper {
        let id = {
            let mut id_guard = NEXT_DATA_PUSHER_ID.lock().await;
            (*id_guard) += 1;
            *id_guard
        };

        let processor = new_ws_processor(
            payload,
            ProcessorMeta {
                watch_dog_timeout_seconds: 5,
                watch_dog_check_interval_seconds: 1,
            },
        );

        WsDataPusherWrapper(processor, id, launched_data_hub)
    }
}

impl WsDataPusherWrapper {
    pub fn launch_inline(
        self,
    ) -> impl Stream<Item = Result<Bytes, super::websocket::processor::ProcessingError<DataPusher>>>
    {
        let (tx, rx) = mpsc::channel::<Action>(8);
        self.0.launch_inline(
            DataPusher {
                id: self.1,
                data_hub: self.2,
                launched_self: LaunchedDataPusher { tx },
            },
            NoSubtask,
            rx,
        )
    }
}

impl Handler<Started> for DataPusher {
    type Output = Result<(), anyhow::Error>;

    async fn handle(&mut self, action: Started) -> Self::Output {
        self.data_hub
            .register_data_pusher(RegisterDataPusher(self.id, self.launched_self.clone()))
            .await
            .context("the mailbox of provided data hub has closed. can't register data pusher")?;

        Ok(())
    }
}

impl ContextHandler<Bytes> for DataPusher {
    type Output = Result<(), anyhow::Error>;

    type Context = WebsocketContext;

    async fn handle_with_context(&mut self, _: &mut Self::Context, _: Bytes) -> Self::Output {
        Err(anyhow::anyhow!("Unexpected data received from the peer"))
    }
}

impl Handler<Stopping> for DataPusher {
    type Output = Result<(), anyhow::Error>;

    async fn handle(&mut self, action: Stopping) -> Self::Output {
        self.data_hub
            .unregister_data_pusher(UnRegisterDataPusher(self.id))
            .await
            .context("the mailbox of provided data hub has closed. can't unregister data pusher")?;

        Ok(())
    }
}

impl ContextHandler<Action> for DataPusher {
    type Output = Result<(), anyhow::Error>;

    type Context = WebsocketContext;

    async fn handle_with_context(
        &mut self,
        context: &mut Self::Context,
        action: Action,
    ) -> Self::Output {
        match action {
            Action::NewData(action) => self.handle_with_context(context, action).await,
            Action::Close(_) => todo!(),
        }
    }
}

impl ContextHandler<NewData> for DataPusher {
    type Output = Result<(), anyhow::Error>;

    type Context = WebsocketContext;

    async fn handle_with_context(
        &mut self,
        context: &mut Self::Context,
        action: NewData,
    ) -> Self::Output {
        let NewData(processor_id, data) = action;
        let bytes = serde_json::to_string(&(processor_id, data))
            .context("failed to serialize the given data into json")?;
        context
            .send_to_peer(ws::Message::Text(bytes.into()))
            .await
            .context("failed to push data")?;

        Ok(())
    }
}

impl ContextHandler<Close> for DataPusher {
    type Output = Result<(), anyhow::Error>;

    type Context = WebsocketContext;

    async fn handle_with_context(&mut self, context: &mut Self::Context, _: Close) -> Self::Output {
        Ok(context.do_close().await)
    }
}
