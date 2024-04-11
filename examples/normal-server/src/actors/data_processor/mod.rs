use super::{
    data_hub::LaunchedDataHub,
    handler::ContextHandler,
    websocket::{
        context::WebsocketContext,
        processor::{
            actions::{ActorAction, NoActions},
            new_ws_processor,
        },
    },
};
use crate::{
    actors::{
        service_broadcast_manager::LaunchedServiceBroadcastManager,
        websocket::processor::{
            actions::{Started, Stopping},
            ProcessorBeforeLaunched, ProcessorMeta,
        },
        Handler,
    },
    entities::{
        data,
        data_transaction::{self, ActiveModel},
    },
};
use actix_web::web::{Bytes, Payload};
use futures::{Future, Stream};
use normal_data::Data;
use sea_orm::{DatabaseConnection, DbErr, Set};
use std::time::Duration;
use tokio::sync::mpsc;

use {
    mutation::Mutation,
    saver::{DataSaver, LaunchedDataSaver},
};

mod actions;
mod id;
mod mutation;
mod saver;

pub use id::*;

#[derive(Debug, thiserror::Error)]
pub enum StartProcessingError {
    #[error("failed to register data processor to the data hub")]
    RegisterProcessorFailed,
    #[error("failed to register the connection to the service broadcast manager due to the closed channel")]
    RegisterConnectionFailed,
}

#[derive(Debug, thiserror::Error)]
pub enum DataProcessingError {
    #[error("the value of the given data is out of range: (id: {}, value: {})", .0, .1)]
    DataValueOutOfRange(u32, u32),
    #[error("failed to save data using the data saver due to timeout")]
    SaveDataTimeout,
    #[error("failed to decode the data from the text frame")]
    DataDecodeFailed(serde_json::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum StopProcessingError {
    #[error("failed to unregister the connection to the service broadcast manager due to the closed channel")]
    UnregisterConnectionFailed,
}

#[derive(Debug)]
pub struct ReceiveDataFromHardware {
    id: DataProcessorId,
    launched_service_broadcast_manager: LaunchedServiceBroadcastManager,
    data_transaction: data_transaction::Model,
    launched_data_saver: LaunchedDataSaver,
    launched_data_hub: LaunchedDataHub,
}

pub struct WsProcessorWrapper<F: Future<Output = Result<(), DbErr>>>(
    ProcessorBeforeLaunched,
    ReceiveDataFromHardware,
    F,
);

impl ReceiveDataFromHardware {
    pub async fn new_ws_processor(
        payload: Payload,
        db_coon: DatabaseConnection,
        launched_service_broadcast_manager: LaunchedServiceBroadcastManager,
        launched_data_hub: LaunchedDataHub,
    ) -> Result<WsProcessorWrapper<impl Future<Output = Result<(), DbErr>>>, DbErr> {
        let data_transaction = Mutation(db_coon.clone())
            .insert_data_transaction(ActiveModel {
                start_time: Set(chrono::Local::now().naive_local()),
                ..Default::default()
            })
            .await?;

        let (launched_data_saver, data_saver_fut) = DataSaver::new(db_coon).launch_inline(Some(60));

        let processor = new_ws_processor(
            payload,
            ProcessorMeta {
                watch_dog_timeout_seconds: 15,
                watch_dog_check_interval_seconds: 1,
            },
        );

        Ok(WsProcessorWrapper(
            processor,
            ReceiveDataFromHardware {
                id: data_transaction.id as u32,
                launched_service_broadcast_manager,
                launched_data_hub,
                data_transaction,
                launched_data_saver,
            },
            data_saver_fut,
        ))
    }
}

impl<F: Future<Output = Result<(), DbErr>>> WsProcessorWrapper<F> {
    pub fn launch_inline(
        self,
    ) -> impl Stream<
        Item = Result<Bytes, super::websocket::processor::ProcessingError<ReceiveDataFromHardware>>,
    > {
        let (_, rx) = mpsc::channel::<NoActions>(1);
        self.0.launch_inline(self.1, self.2, rx)
    }
}

impl Handler<Started> for ReceiveDataFromHardware {
    type Output = Result<(), StartProcessingError>;

    async fn handle(&mut self, _action: Started) -> Self::Output {
        self.launched_data_hub
            .register_data_processor(self.id)
            .await
            .map_err(|_| StartProcessingError::RegisterProcessorFailed)?;

        self.launched_service_broadcast_manager
            .register_connection()
            .await
            .map_err(|_| StartProcessingError::RegisterConnectionFailed)
    }
}

impl ContextHandler<Bytes> for ReceiveDataFromHardware {
    type Output = Result<(), DataProcessingError>;
    type Context = WebsocketContext;

    async fn handle_with_context(
        &mut self,
        _context: &mut Self::Context,
        bytes: Bytes,
    ) -> Self::Output {
        let data: Data =
            serde_json::from_slice(&bytes[..]).map_err(DataProcessingError::DataDecodeFailed)?;

        log::trace!("data: {:?}", data);

        self.launched_data_hub
            .new_data_from_processor(self.id, data.clone())
            .await
            .expect("failed to send data to the data hub");

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
            .map_err(|_| DataProcessingError::SaveDataTimeout)
    }
}

impl Handler<Stopping> for ReceiveDataFromHardware {
    type Output = Result<(), StopProcessingError>;

    async fn handle(&mut self, _action: Stopping) -> Self::Output {
        self.launched_data_hub
            .unregister_data_processor(self.id)
            .await
            .expect("failed to unregister this processor");

        self.launched_service_broadcast_manager
            .unregister_connection()
            .await
            .map_err(|_| StopProcessingError::UnregisterConnectionFailed)
    }
}

impl ContextHandler<NoActions> for ReceiveDataFromHardware {
    type Output = anyhow::Result<ActorAction>;

    type Context = WebsocketContext;

    async fn handle_with_context(&mut self, _: &mut Self::Context, _: NoActions) -> Self::Output {
        unreachable!()
    }
}
