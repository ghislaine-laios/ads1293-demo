use super::{
    mutation::Mutation,
    saver::{DataSaver, LaunchedDataSaver},
};
use crate::{
    actors::{
        service_broadcast_manager::LaunchedServiceBroadcastManager,
        websocket::processor::{
            actions::{Started, Stopping},
            Processor, ProcessorBeforeLaunched, ProcessorMeta,
        },
        Handler,
    },
    entities::{
        data,
        data_transaction::{self, ActiveModel},
    },
};
use actix_web::web::Payload;
use futures::Future;
use normal_data::Data;
use sea_orm::{DatabaseConnection, DbErr, Set};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum StartProcessingError {
    #[error("failed to register the connection to the service broadcast manager due to the closed channel")]
    RegisterConnectionFailed,
}

#[derive(Debug, thiserror::Error)]
pub enum DataProcessingError {
    #[error("the value of the given data is out of range: (id: {}, value: {})", .0, .1)]
    DataValueOutOfRange(u32, u32),
    #[error("failed to save data using the data saver due to timeout")]
    SaveDataTimeout,
}

#[derive(Debug, thiserror::Error)]
pub enum StopProcessingError {
    #[error("failed to unregister the connection to the service broadcast manager due to the closed channel")]
    UnregisterConnectionFailed,
}

#[derive(Debug)]
pub struct ReceiveDataFromHardware {
    launched_service_broadcast_manager: LaunchedServiceBroadcastManager,
    data_transaction: data_transaction::Model,
    launched_data_saver: LaunchedDataSaver,
}

impl ReceiveDataFromHardware {
    pub async fn new_ws_processor(
        payload: Payload,
        db_coon: DatabaseConnection,
        launched_service_broadcast_manager: LaunchedServiceBroadcastManager,
    ) -> Result<
        ProcessorBeforeLaunched<ReceiveDataFromHardware, impl Future<Output = Result<(), DbErr>>>,
        DbErr,
    > {
        let data_transaction = Mutation(db_coon.clone())
            .insert_data_transaction(ActiveModel {
                start_time: Set(chrono::Local::now().naive_local()),
                ..Default::default()
            })
            .await?;

        let (launched_data_saver, data_saver_fut) = DataSaver::new(db_coon).launch_inline(Some(60));

        Ok(Processor::new::<15, 1>(
            payload,
            ProcessorMeta {
                process_data_handler: ReceiveDataFromHardware {
                    launched_service_broadcast_manager,
                    data_transaction,
                    launched_data_saver,
                },
                subtask: data_saver_fut,
            },
        ))
    }
}

impl Handler<Started> for ReceiveDataFromHardware {
    type Output = Result<(), StartProcessingError>;

    async fn handle(&mut self, _action: Started) -> Self::Output {
        self.launched_service_broadcast_manager
            .register_connection()
            .await
            .map_err(|_| StartProcessingError::RegisterConnectionFailed)
    }
}

impl Handler<Data> for ReceiveDataFromHardware {
    type Output = Result<(), DataProcessingError>;

    async fn handle(&mut self, data: Data) -> Self::Output {
        log::trace!("data: {:?}", data);

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
        self.launched_service_broadcast_manager
            .unregister_connection()
            .await
            .map_err(|_| StopProcessingError::UnregisterConnectionFailed)
    }
}
