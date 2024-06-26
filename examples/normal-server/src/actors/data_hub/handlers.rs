use std::mem::replace;

use anyhow::{anyhow, Context};
use sea_orm::Set;

use crate::{
    actors::{data_processor::mutation::Mutation, Handler},
    entities::data,
};

use super::{
    actions::{self, Action, ControlAction, NewDataFromProcessor},
    DataHub,
};

macro_rules! dispatch {
    ($self:ident, $action:ident) => {
        $self.handle($action).await?
    };
}

impl Handler<Action> for DataHub {
    type Output = anyhow::Result<()>;

    async fn handle(&mut self, action: Action) -> Self::Output {
        log::trace!("action: {:?}", action);

        match action {
            Action::NewDataFromProcessor(action) => dispatch!(self, action),
        }

        Ok(())
    }
}

impl Handler<ControlAction> for DataHub {
    type Output = anyhow::Result<()>;

    async fn handle(&mut self, action: ControlAction) -> Self::Output {
        macro_rules! self_dispatch {
            ($action:ident) => {
                dispatch!(self, $action)
            };
        }

        match action {
            ControlAction::RegisterDataProcessor(action) => self_dispatch!(action),
            ControlAction::UnregisterDataProcessor(action) => self_dispatch!(action),
            ControlAction::RegisterDataPusher(action) => self_dispatch!(action),
            ControlAction::UnRegisterDataPusher(action) => self_dispatch!(action),
        }

        Ok(())
    }
}

impl Handler<actions::RegisterDataProcessor> for DataHub {
    type Output = anyhow::Result<()>;

    async fn handle(&mut self, action: actions::RegisterDataProcessor) -> Self::Output {
        if !self.data_processors.insert(action.0) {
            return Err(anyhow::anyhow!(
                "there exists a registered data processor which has the same id ({})",
                action.0
            ));
        }

        Ok(())
    }
}

impl Handler<actions::UnregisterDataProcessor> for DataHub {
    type Output = anyhow::Result<()>;

    async fn handle(&mut self, action: actions::UnregisterDataProcessor) -> Self::Output {
        if !self.data_processors.remove(&action.0) {
            return Err(anyhow::anyhow!(
                "the data processor (id: {}) requested to be unregistered has not been registered",
                action.0
            ));
        }

        Ok(())
    }
}

impl Handler<actions::RegisterDataPusher> for DataHub {
    type Output = anyhow::Result<()>;

    async fn handle(&mut self, action: actions::RegisterDataPusher) -> Self::Output {
        if let Some(pusher) = self.data_pusher.as_ref() {
            log::debug!(
                "There exists a registered data pusher (id: {}). Replace it.",
                pusher.0
            );
            pusher.1.close().await.context(format!(
                "when closing the data pusher of id {} we found that the channel has been closed",
                pusher.0
            ))?;
        };

        let outdated = self.data_pusher.replace((action.0, action.1));
        log::trace!("outdated: {:?}", outdated);
        if outdated.is_some() && self.outdated_data_pusher.is_some() {
            return Err(anyhow!("There is already an outdated_data_pusher waiting to be unregistered, so we can't replace the current data pusher"));
        }

        self.outdated_data_pusher = outdated;

        log::debug!("Successfully register new data pusher.");
        log::trace!(
            "Current data pusher: {:#?}\nOutdated data pusher: {:#?}",
            self.data_pusher,
            self.outdated_data_pusher
        );

        Ok(())
    }
}

impl Handler<actions::UnRegisterDataPusher> for DataHub {
    type Output = anyhow::Result<()>;

    async fn handle(&mut self, action: actions::UnRegisterDataPusher) -> Self::Output {
        let err = Err(anyhow::anyhow!(
            "the data pusher (id: {}) requested to be unregistered has not been registered",
            action.0
        ));

        if self
            .data_pusher
            .as_ref()
            .is_some_and(|pusher| pusher.0 == action.0)
        {
            log::debug!(
                "Data pusher has been unregistered. id = {}",
                self.data_pusher.as_ref().unwrap().0
            );

            self.data_pusher = None;
        } else if self
            .outdated_data_pusher
            .as_ref()
            .is_some_and(|pusher| pusher.0 == action.0)
        {
            log::debug!(
                "Outdated data pusher has been unregistered. id = {}",
                self.outdated_data_pusher.as_ref().unwrap().0
            );

            self.outdated_data_pusher = None;
        } else {
            return err;
        }

        Ok(())
    }
}

impl Handler<actions::NewDataFromProcessor> for DataHub {
    type Output = anyhow::Result<()>;

    async fn handle(&mut self, action: actions::NewDataFromProcessor) -> Self::Output {
        log::trace!("new data from processor: {:?}", action);
        let NewDataFromProcessor(data_processor_id, data) = action;

        let model = data::ActiveModel {
            data_transaction_id: Set(data_processor_id as i64),
            id: Set(data.id.into()),
            ecg1: Set(data.ecg.0 as i32),
            ecg2: Set(data.ecg.1 as i32),
            quaternion: Set(serde_json::to_value(&data.quaternion).unwrap()),
            accel: Set(serde_json::to_value(&data.accel).unwrap()),
        };

        self.data_buffer.push(model);

        if self.data_buffer.len() >= 20 {
            let buffer = replace(&mut self.data_buffer, Vec::with_capacity(20));
            self.mutation
                .bulk_insert_data(buffer.into_iter())
                .await
                .unwrap();
        }

        if let Some((id, pusher)) = self.data_pusher.as_ref() {
            // TODO: Refactor this
            let _ = pusher
                .send_data(data_processor_id, data)
                .await
                .context(format!(
                    "failed to send data to the data pusher (id = {})",
                    id
                ))
                .map_err(|e| log::warn!(e:?;"failed to send data to the data pusher"));
        }

        Ok(())
    }
}
