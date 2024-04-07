use actix_web::web::Payload;
use normal_data::Data;

use crate::actors::{
    service_broadcast_manager::LaunchedServiceBroadcastManager,
    websocket::processor::{
        actions::{Started, Stopping},
        Processor, ProcessorBeforeLaunched, ProcessorMeta,
    },
    Handler,
};

#[derive(Debug, thiserror::Error)]
pub enum StartProcessingError {
    #[error("failed to register the connection to the service broadcast manager due to the closed channel")]
    RegisterConnectionFailed,
}

#[derive(Debug, thiserror::Error)]
pub enum DataProcessingError {}

#[derive(Debug, thiserror::Error)]
pub enum StopProcessingError {
    #[error("failed to unregister the connection to the service broadcast manager due to the closed channel")]
    UnregisterConnectionFailed,
}

#[derive(Debug)]
pub struct ReceiveDataFromHardware {
    launched_service_broadcast_manager: LaunchedServiceBroadcastManager,
}

impl ReceiveDataFromHardware {
    pub fn new_ws_processor(
        payload: Payload,
        launched_service_broadcast_manager: LaunchedServiceBroadcastManager,
    ) -> ProcessorBeforeLaunched<
        ReceiveDataFromHardware,
        StartProcessingError,
        DataProcessingError,
        StopProcessingError,
    > {
        Processor::new::<15, 1>(
            payload,
            ProcessorMeta {
                process_data_handler: ReceiveDataFromHardware {
                    launched_service_broadcast_manager,
                },
            },
        )
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
        log::info!("{:?}", data);

        Ok(())
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
