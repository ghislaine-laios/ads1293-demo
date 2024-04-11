use normal_data::Data;
use tokio::sync::mpsc;

use crate::actors::data_processor::DataProcessorId;

use super::actions::{Action, Close, NewData};

#[derive(Debug, Clone)]
pub struct LaunchedDataPusher {
    pub(super) tx: mpsc::Sender<Action>,
}

impl LaunchedDataPusher {
    pub async fn send_data(
        &self,
        processor_id: DataProcessorId,
        data: Data,
    ) -> Result<(), mpsc::error::SendError<Action>> {
        self.tx
            .send(Action::NewData(NewData(processor_id, data)))
            .await
    }

    pub async fn close(&self) -> Result<(), mpsc::error::SendError<Action>> {
        self.tx.send(Action::Close(Close)).await
    }
}
