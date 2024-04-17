use tokio::sync::mpsc;

use crate::actors::{
    data_processor::DataProcessorId,
    data_pusher::{DataPusherId, LaunchedDataPusher},
};

use super::{actions::ControlAction, LaunchedDataHubController};

#[derive(Debug)]
pub struct DataProcessorRegistrationKeeper {
    id: DataProcessorId,
    controller: LaunchedDataHubController,
}

impl DataProcessorRegistrationKeeper {
    pub fn new(
        id: DataProcessorId,
        controller: LaunchedDataHubController,
    ) -> Result<Self, mpsc::error::SendError<ControlAction>> {
        controller.register_data_processor(id)?;
        Ok(Self { id, controller })
    }
}

impl Drop for DataProcessorRegistrationKeeper {
    fn drop(&mut self) {
        let _ = self
            .controller
            .unregister_data_processor(self.id)
            .map_err(|e| {
                log::error! {
                    data_processor_id:? = self.id, error:? = e, err_msg:err = e;
                    "Failed to unregister the data processor due to closed channel when dropping a data processor."
                }
            });
    }
}

#[derive(Debug)]
pub struct DataPusherRegistrationKeeper {
    id: DataPusherId,
    controller: LaunchedDataHubController,
}

impl DataPusherRegistrationKeeper {
    pub fn new(
        id: DataPusherId,
        controller: LaunchedDataHubController,
        launched_data_pusher: LaunchedDataPusher,
    ) -> Result<Self, mpsc::error::SendError<ControlAction>> {
        controller.register_data_pusher(id, launched_data_pusher)?;

        Ok(Self { id, controller })
    }
}

impl Drop for DataPusherRegistrationKeeper {
    fn drop(&mut self) {
        let _ = self.controller.unregister_data_pusher(self.id).map_err(|e| {
            log::error! {
                data_pusher_id:? = self.id, error:? = e, err_msg:err = e;
                "Failed to unregister the data pusher due to closed channel when dropping a data pusher."
            };
        });
    }
}
