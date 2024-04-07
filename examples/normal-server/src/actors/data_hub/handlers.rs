use anyhow::Context;

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
            return Err(anyhow::anyhow!(
                "there exists a registered data pusher (id: {})",
                pusher.0
            ));
        }
        self.data_pusher = Some((action.0, action.1));

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

        let Some(pusher) = self.data_pusher.as_ref() else {
            return err;
        };

        if pusher.0 != action.0 {
            return err;
        };

        self.data_pusher = None;

        Ok(())
    }
}

impl Handler<actions::NewDataFromProcessor> for DataHub {
    type Output = anyhow::Result<()>;

    async fn handle(&mut self, action: actions::NewDataFromProcessor) -> Self::Output {
        let NewDataFromProcessor(data_processor_id, data) = action;
        if let Some((_, pusher)) = self.data_pusher.as_ref() {
            pusher
                .send_data(data_processor_id, data)
                .await
                .context("failed to send data to the data pusher")?;
        }

        Ok(())
    }
}
