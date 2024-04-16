use actix_http::ws;
use anyhow::Context;

use crate::actors::websocket::{
    actor_context::{EventLoopInstruction, WebsocketActorContextHandler},
    WebsocketContext,
};

use super::{
    actions::{Action, Close, NewData},
    DataPusher,
};

impl WebsocketActorContextHandler for DataPusher {
    type Action = Action;

    async fn started(&mut self, _: &mut WebsocketContext) -> anyhow::Result<()> {
        self.data_hub
            .register_data_pusher(self.id, self.launched_self.clone())
            .await
            .context("the mailbox of provided data hub has closed. can't register data pusher")
    }

    async fn handle_action_with_context(
        &mut self,
        context: &mut WebsocketContext,
        action: Self::Action,
    ) -> anyhow::Result<crate::actors::websocket::actor_context::EventLoopInstruction> {
        log::trace!(action:?; "Received new action.");
        match action {
            Action::NewData(action) => self.handle_new_data(context, action).await,
            Action::Close(action) => self.handle_close(context, action).await,
        }
    }

    async fn handle_bytes_with_context(
        &mut self,
        context: &mut WebsocketContext,
        bytes: actix_web::web::Bytes,
    ) -> anyhow::Result<crate::actors::websocket::actor_context::EventLoopInstruction> {
        log::debug!(bytes:?; "Received data from the frontend.");

        if bytes.slice(..) == "ping" {
            context
                .send_to_peer(ws::Message::Text("pong".into()))
                .await?;
            Ok(crate::actors::websocket::actor_context::EventLoopInstruction::Continue)
        } else {
            Err(anyhow::anyhow!(
                "Unexpected data received from the peer: {:?}",
                bytes
            ))
        }
    }

    fn get_id(&self) -> u64 {
        self.id as u64
    }
}

impl DataPusher {
    async fn handle_new_data(
        &mut self,
        context: &mut WebsocketContext,
        action: NewData,
    ) -> anyhow::Result<EventLoopInstruction> {
        const FAILED_DECODE_STR: &'static str = "Failed to serialize the given data into json.";
        const FAILED_PUSH_STR: &'static str = "Failed to push the data to the frontend.";

        let NewData(processor_id, data) = action;

        log::trace! {
            data_pusher_id:serde = self.id, processor_id:serde, data:serde;
            "Received new data from a processor."
        };

        let tuple = (processor_id, &data);

        let bytes = serde_json::to_string(&tuple)
            .context(FAILED_DECODE_STR)
            .map_err(|e| {
                log::error! {
                    data_pusher_id:serde = self.id, processor_id:serde, data:serde, error:debug = e;
                    "{}", FAILED_DECODE_STR
                };
                e
            })?;

        let frame = ws::Message::Text(bytes.into());

        log::trace! {
            data_pusher_id:serde = self.id, processor_id:serde, data:serde, frame:debug;
            "Frame generated."
        }

        context
            .send_to_peer(frame)
            .await
            .context(FAILED_PUSH_STR)
            .map_err(|e| {
                log::error! {
                    data_pusher_id:serde = self.id, processor_id:serde, data:serde, error:debug = e;
                    "{}", FAILED_PUSH_STR
                };
                e
            })?;

        log::trace! {
            data_pusher_id:serde = self.id, data:serde, processor_id:serde;
            "Pushed the data to the frontend."
        }

        Ok(EventLoopInstruction::Continue)
    }

    async fn handle_close(
        &mut self,
        _context: &mut WebsocketContext,
        _: Close,
    ) -> anyhow::Result<EventLoopInstruction> {
        log::trace! {
            data_pusher_id:serde = self.id;
            "A data pusher IS requested to be closed"
        }

        Ok(EventLoopInstruction::Break)
    }
}

impl Drop for DataPusher {
    fn drop(&mut self) {
        self.data_hub.try_unregister_data_pusher(self.id).unwrap()
    }
}
