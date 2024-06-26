use super::{
    data_hub::{
        registration_keepers::DataProcessorRegistrationKeeper, LaunchedDataHub,
        LaunchedDataHubController,
    },
    service_broadcast_manager::{self, ConnectionKeeper},
    websocket::actor_context::WebsocketActorContext,
};
use crate::{
    actors::service_broadcast_manager::LaunchedServiceBroadcastManager,
    entities::data_transaction::{self, ActiveModel},
};
use actix_web::web::{Bytes, Payload};
use anyhow::Context;
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
pub mod mutation;
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
    data_transaction: data_transaction::Model,
    launched_data_saver: LaunchedDataSaver,
    launched_data_hub: LaunchedDataHub,
    #[allow(unused)]
    connection_keeper: service_broadcast_manager::ConnectionKeeper,
    #[allow(unused)]
    registration_keeper: DataProcessorRegistrationKeeper,
}

impl ReceiveDataFromHardware {
    pub async fn launch_inline(
        payload: Payload,
        db_coon: DatabaseConnection,
        launched_service_broadcast_manager: LaunchedServiceBroadcastManager,
        launched_data_hub: LaunchedDataHub,
        data_hub_controller: LaunchedDataHubController,
    ) -> Result<
        impl Stream<Item = Result<Bytes, super::websocket::actor_context::TaskExecutionError>>,
        anyhow::Error,
    > {
        const FAILED_TO_INSERT_DATA_TRANSACTION_STR: &'static str =
            "failed to insert the data transaction into the database";
        const FAILED_TO_CREATE_CONNECTION_KEEPER_STR: &'static str = "failed to register the connection to the service broadcast manager due to the closed channel";
        const FAILED_TO_CREATE_REGISTRATION_KEEPER_STR: &'static str =
            "failed to register the data processor to the data hub due to the closed channel";

        let data_transaction = Mutation(db_coon.clone())
            .insert_data_transaction(ActiveModel {
                start_time: Set(chrono::Local::now().naive_local()),
                ..Default::default()
            })
            .await
            .context(FAILED_TO_INSERT_DATA_TRANSACTION_STR)?;

        let (launched_data_saver, data_saver_fut) = DataSaver::new(db_coon).launch_inline(Some(60));

        let (_, action_rx) = mpsc::channel(1);

        let id = data_transaction.id as u32;

        Ok(WebsocketActorContext::launch_inline(
            payload,
            action_rx,
            Self {
                id,
                data_transaction,
                launched_data_saver,
                launched_data_hub,
                connection_keeper: ConnectionKeeper::new(launched_service_broadcast_manager)
                    .context(FAILED_TO_CREATE_CONNECTION_KEEPER_STR)?,
                registration_keeper: DataProcessorRegistrationKeeper::new(id, data_hub_controller)
                    .context(FAILED_TO_CREATE_REGISTRATION_KEEPER_STR)?,
            },
            Duration::from_secs(15),
            Duration::from_secs(1),
            data_saver_fut.map_err(|e| e.into()),
        ))
    }
}
