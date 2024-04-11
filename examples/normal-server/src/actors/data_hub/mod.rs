use self::actions::{
    Action, NewDataFromProcessor, RegisterDataProcessor, RegisterDataPusher, UnRegisterDataPusher,
    UnregisterDataProcessor,
};
use super::{
    data_processor::DataProcessorId,
    data_pusher::{DataPusherId, LaunchedDataPusher},
    Handler,
};
use normal_data::Data;
use std::collections::HashSet;
use tokio::sync::mpsc;

pub mod actions;
pub mod handlers;

#[derive(Default)]
pub struct DataHub {
    data_processors: HashSet<DataProcessorId>,
    data_pusher: Option<(DataPusherId, LaunchedDataPusher)>,
    outdated_data_pusher: Option<(DataPusherId, LaunchedDataPusher)>
}

impl DataHub {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn launch(self) -> (LaunchedDataHub, tokio::task::JoinHandle<anyhow::Result<()>>) {
        let (tx, rx) = tokio::sync::mpsc::channel::<actions::Action>(4);
        let join_handle = actix_rt::spawn(async move { self.task(rx).await });

        (LaunchedDataHub { tx }, join_handle)
    }

    async fn task(mut self, mut rx: mpsc::Receiver<actions::Action>) -> anyhow::Result<()> {
        while let Some(action) = rx.recv().await {
            self.handle(action).await?;
        }

        Ok(())
    }
}

impl Drop for DataHub {
    fn drop(&mut self) {
        log::debug!("A data hub is dropped")
    }
}

#[derive(Clone, Debug)]
pub struct LaunchedDataHub {
    tx: mpsc::Sender<actions::Action>,
}

impl LaunchedDataHub {
    pub async fn register_data_processor(
        &self,
        id: DataProcessorId,
    ) -> Result<(), mpsc::error::SendError<Action>> {
        self.tx
            .send(Action::RegisterDataProcessor(RegisterDataProcessor(id)))
            .await
    }

    pub async fn unregister_data_processor(
        &self,
        id: DataProcessorId,
    ) -> Result<(), mpsc::error::SendError<Action>> {
        self.tx
            .send(Action::UnregisterDataProcessor(UnregisterDataProcessor(id)))
            .await
    }

    pub async fn register_data_pusher(
        &self,
        action: RegisterDataPusher,
    ) -> Result<(), mpsc::error::SendError<Action>> {
        self.tx.send(Action::RegisterDataPusher(action)).await
    }

    pub async fn unregister_data_pusher(
        &self,
        action: UnRegisterDataPusher,
    ) -> Result<(), mpsc::error::SendError<Action>> {
        self.tx.send(Action::UnRegisterDataPusher(action)).await
    }

    pub async fn new_data_from_processor(
        &self,
        processor_id: DataProcessorId,
        data: Data,
    ) -> Result<(), mpsc::error::SendError<Action>> {
        self.tx
            .send(Action::NewDataFromProcessor(NewDataFromProcessor(
                processor_id,
                data,
            )))
            .await
    }
}
