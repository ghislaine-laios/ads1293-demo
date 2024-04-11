use super::{data_hub::LaunchedDataHub, websocket::neo::WebsocketActorContext};
use crate::{
    actors::service_broadcast_manager::LaunchedServiceBroadcastManager,
    entities::data_transaction::{self, ActiveModel},
};
use actix_web::web::{Bytes, Payload};
use futures::{Stream, TryFutureExt};
use sea_orm::{DatabaseConnection, Set};
use std::time::Duration;
use tokio::sync::mpsc;

use {
    mutation::Mutation,
    saver::{DataSaver, LaunchedDataSaver},
};

mod actions;
mod handler;
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

impl ReceiveDataFromHardware {
    pub async fn launch_inline(
        payload: Payload,
        db_coon: DatabaseConnection,
        launched_service_broadcast_manager: LaunchedServiceBroadcastManager,
        launched_data_hub: LaunchedDataHub,
    ) -> Result<
        impl Stream<Item = Result<Bytes, super::websocket::neo::TaskExecutionError>>,
        anyhow::Error,
    > {
        let data_transaction = Mutation(db_coon.clone())
            .insert_data_transaction(ActiveModel {
                start_time: Set(chrono::Local::now().naive_local()),
                ..Default::default()
            })
            .await?;

        let (launched_data_saver, data_saver_fut) = DataSaver::new(db_coon).launch_inline(Some(60));

        let (_, action_rx) = mpsc::channel(1);

        Ok(WebsocketActorContext::launch_inline(
            payload,
            action_rx,
            Self {
                id: data_transaction.id as u32,
                launched_service_broadcast_manager,
                data_transaction,
                launched_data_saver,
                launched_data_hub,
            },
            Duration::from_secs(15),
            Duration::from_secs(1),
            data_saver_fut.map_err(|e| e.into()),
        ))
    }
}
