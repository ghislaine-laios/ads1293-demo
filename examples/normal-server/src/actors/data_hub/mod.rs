use self::actions::{
    Action, ControlAction, NewDataFromProcessor, RegisterDataProcessor, RegisterDataPusher,
    UnRegisterDataPusher, UnregisterDataProcessor,
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
pub mod registration_keepers;

pub const MODULE_PATH: &'static str = module_path!();

#[derive(Default)]
pub struct DataHub {
    data_processors: HashSet<DataProcessorId>,
    data_pusher: Option<(DataPusherId, LaunchedDataPusher)>,
    outdated_data_pusher: Option<(DataPusherId, LaunchedDataPusher)>,
}

impl DataHub {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn launch(
        self,
    ) -> (
        LaunchedDataHub,
        LaunchedDataHubController,
        tokio::task::JoinHandle<Result<(), anyhow::Error>>,
    ) {
        let (tx, rx) = mpsc::channel::<actions::Action>(8);
        let (control_tx, control_rx) = mpsc::unbounded_channel();
        let join_handle = actix_rt::spawn(async move { self.task(rx, control_rx).await });

        (
            LaunchedDataHub { tx },
            LaunchedDataHubController { control_tx },
            join_handle,
        )
    }

    async fn task(
        mut self,
        mut rx: mpsc::Receiver<actions::Action>,
        mut control_action_rx: mpsc::UnboundedReceiver<actions::ControlAction>,
    ) -> anyhow::Result<()> {
        macro_rules! process_action {
            ($action:ident) => {{
                let Some(action) = $action else { break };
                self.handle(action).await?;
            }};
        }

        loop {
            tokio::select! {
                biased;

                action = control_action_rx.recv() => {
                    process_action!(action)
                }
                action = rx.recv() => {
                    process_action!(action)
                }

            }
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

#[derive(Clone, Debug)]
pub struct LaunchedDataHubController {
    control_tx: mpsc::UnboundedSender<actions::ControlAction>,
}

impl LaunchedDataHub {
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

impl LaunchedDataHubController {
    pub fn register_data_processor(
        &self,
        id: DataProcessorId,
    ) -> Result<(), mpsc::error::SendError<ControlAction>> {
        self.control_tx
            .send(ControlAction::RegisterDataProcessor(RegisterDataProcessor(
                id,
            )))
    }

    pub fn unregister_data_processor(
        &self,
        id: DataProcessorId,
    ) -> Result<(), mpsc::error::SendError<ControlAction>> {
        self.control_tx.send(ControlAction::UnregisterDataProcessor(
            UnregisterDataProcessor(id),
        ))
    }

    pub fn register_data_pusher(
        &self,
        id: DataPusherId,
        launched: LaunchedDataPusher,
    ) -> Result<(), mpsc::error::SendError<ControlAction>> {
        self.control_tx
            .send(ControlAction::RegisterDataPusher(RegisterDataPusher(
                id, launched,
            )))
    }

    pub fn unregister_data_pusher(
        &self,
        id: DataPusherId,
    ) -> Result<(), mpsc::error::SendError<ControlAction>> {
        self.control_tx
            .send(ControlAction::UnRegisterDataPusher(UnRegisterDataPusher(
                id,
            )))
    }
}
