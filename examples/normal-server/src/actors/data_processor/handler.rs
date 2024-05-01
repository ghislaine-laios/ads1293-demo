use std::time::Duration;

use anyhow::Context;
use normal_data::Data;
use sea_orm::Set;

use crate::{
    actors::websocket::actor_context::{NoAction, WebsocketActorContextHandler},
    entities::data,
};

use super::{DataProcessingError, ReceiveDataFromHardware};

impl WebsocketActorContextHandler for ReceiveDataFromHardware {
    type Action = NoAction;

    async fn started(
        &mut self,
        _: &mut crate::actors::websocket::WebsocketContext,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn stopped(
        &mut self,
        _: &mut crate::actors::websocket::WebsocketContext,
        error: Option<crate::actors::websocket::actor_context::TaskExecutionError>,
    ) -> anyhow::Result<Option<crate::actors::websocket::actor_context::TaskExecutionError>> {
        Ok(error)
    }

    async fn handle_action_with_context(
        &mut self,
        _: &mut crate::actors::websocket::WebsocketContext,
        _: Self::Action,
    ) -> anyhow::Result<crate::actors::websocket::actor_context::EventLoopInstruction> {
        unreachable!()
    }

    async fn handle_bytes_with_context(
        &mut self,
        _: &mut crate::actors::websocket::WebsocketContext,
        bytes: actix_web::web::Bytes,
    ) -> anyhow::Result<crate::actors::websocket::actor_context::EventLoopInstruction> {
        let data: Data =
            serde_json::from_slice(&bytes[..]).map_err(DataProcessingError::DataDecodeFailed)?;

        log::trace!(data:serde; "Handle new data from the hardware");

        self.launched_data_saver
            .save_timeout(
                data::ActiveModel {
                    data_transaction_id: Set(self.data_transaction.id),
                    id: Set(data.id.into()),
                    value: Set(data.ecg.try_into().map_err(|_| {
                        DataProcessingError::DataValueOutOfRange(data.id, data.ecg)
                    })?),
                },
                Duration::from_millis(200),
            )
            .await
            .map_err(|_| DataProcessingError::SaveDataTimeout)?;

        self.launched_data_hub
            .new_data_from_processor(self.id, data)
            .await
            .context("failed to send data to the data hub.")?;

        Ok(crate::actors::websocket::actor_context::EventLoopInstruction::Continue)
    }

    fn get_id(&self) -> u64 {
        self.id as u64
    }
}
