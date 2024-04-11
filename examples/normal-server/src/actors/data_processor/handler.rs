use std::time::Duration;

use anyhow::Context;
use normal_data::Data;
use sea_orm::Set;

use crate::{
    actors::{
        data_processor::StopProcessingError,
        websocket::neo::{NoAction, WebsocketActorContextHandler},
    },
    entities::data,
};

use super::{DataProcessingError, ReceiveDataFromHardware, StartProcessingError};

impl WebsocketActorContextHandler for ReceiveDataFromHardware {
    type Action = NoAction;

    async fn started(
        &mut self,
        _: &mut crate::actors::websocket::neo::websocket_context::WebsocketContext,
    ) -> anyhow::Result<()> {
        self.launched_data_hub
            .register_data_processor(self.id)
            .await
            .map_err(|_| StartProcessingError::RegisterProcessorFailed)?;

        self.launched_service_broadcast_manager
            .register_connection()
            .await
            .map_err(|_| StartProcessingError::RegisterConnectionFailed)?;

        Ok(())
    }

    async fn stopped(
        &mut self,
        _: &mut crate::actors::websocket::neo::websocket_context::WebsocketContext,
        error: Option<crate::actors::websocket::neo::TaskExecutionError>,
    ) -> anyhow::Result<Option<crate::actors::websocket::neo::TaskExecutionError>> {
        self.launched_data_hub
            .unregister_data_processor(self.id)
            .await
            .context("failed to unregister this processor in data hub")?;

        self.launched_service_broadcast_manager
            .unregister_connection()
            .await
            .map_err(|_| StopProcessingError::UnregisterConnectionFailed)?;

        Ok(error)
    }

    async fn handle_action_with_context(
        &mut self,
        _: &mut crate::actors::websocket::neo::websocket_context::WebsocketContext,
        _: Self::Action,
    ) -> anyhow::Result<crate::actors::websocket::neo::EventLoopInstruction> {
        unreachable!()
    }

    async fn handle_bytes_with_context(
        &mut self,
        _: &mut crate::actors::websocket::neo::websocket_context::WebsocketContext,
        bytes: actix_web::web::Bytes,
    ) -> anyhow::Result<crate::actors::websocket::neo::EventLoopInstruction> {
        let data: Data =
            serde_json::from_slice(&bytes[..]).map_err(DataProcessingError::DataDecodeFailed)?;

        log::trace!(data:serde; "Handle new data from the hardware");

        self.launched_data_saver
            .save_timeout(
                data::ActiveModel {
                    data_transaction_id: Set(self.data_transaction.id),
                    id: Set(data.id.into()),
                    value: Set(data.value.try_into().map_err(|_| {
                        DataProcessingError::DataValueOutOfRange(data.id, data.value)
                    })?),
                },
                Duration::from_millis(200),
            )
            .await
            .map_err(|_| DataProcessingError::SaveDataTimeout)?;

        Ok(crate::actors::websocket::neo::EventLoopInstruction::Continue)
    }

    fn get_id(&self) -> u64 {
        self.id as u64
    }
}
