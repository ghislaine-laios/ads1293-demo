use anyhow::{anyhow, Context};

use crate::actors::Handler;

use super::{
    actions::{self, Action, NewDataFromProcessor},
    DataHub,
};

impl Handler<Action> for DataHub {
    type Output = anyhow::Result<()>;

    async fn handle(&mut self, action: Action) -> Self::Output {
        log::trace!("action: {:?}", action);

        match action {
            Action::RegisterDataProcessor(action) => self.handle(action).await?,
            Action::UnregisterDataProcessor(action) => self.handle(action).await?,
            Action::RegisterDataPusher(action) => self.handle(action).await?,
            Action::UnRegisterDataPusher(action) => self.handle(action).await?,
            Action::NewDataFromProcessor(action) => self.handle(action).await?,
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
        if let Some((id, pusher)) = self.data_pusher.as_ref() {
            pusher
                .send_data(data_processor_id, data)
                .await
                .context(format!(
                    "failed to send data to the data pusher (id = {})",
                    id
                ))?;
        }

        Ok(())
    }
}
