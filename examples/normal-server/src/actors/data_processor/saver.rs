use std::time::Duration;

use sea_orm::{DatabaseConnection, DbErr};

use crate::{actors::Handler, entities::data};

use self::actions::Save;

use super::mutation::Mutation;

const FLUSH_TIME: usize = 10;

pub struct DataSaver {
    buf: Vec<data::ActiveModel>,
    mutation: Mutation,
}

impl DataSaver {
    pub fn new(db_coon: DatabaseConnection) -> Self {
        Self {
            buf: Vec::with_capacity(FLUSH_TIME),
            mutation: Mutation(db_coon),
        }
    }

    pub fn launch(
        self,
    ) -> (
        LaunchedDataSaver,
        tokio::task::JoinHandle<Result<(), DbErr>>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::channel(10);
        let join_handle = actix_rt::spawn(async move { self.task(rx).await });

        (LaunchedDataSaver { tx }, join_handle)
    }

    async fn task(
        mut self,
        mut rx: tokio::sync::mpsc::Receiver<actions::Action>,
    ) -> Result<(), DbErr> {
        while let Some(action) = rx.recv().await {
            self.process_action(action).await?;
        }

        Ok(())
    }

    async fn process_action(&mut self, action: actions::Action) -> Result<(), DbErr> {
        let actions::Action::Save(action) = action;
        self.handle(action).await
    }
}

pub struct LaunchedDataSaver {
    tx: tokio::sync::mpsc::Sender<actions::Action>,
}

impl LaunchedDataSaver {
    pub async fn save_timeout(
        &self,
        data: data::ActiveModel,
        timeout: Duration,
    ) -> Result<(), tokio::sync::mpsc::error::SendTimeoutError<actions::Action>> {
        self.tx
            .send_timeout(actions::Action::Save(Save(data)), timeout)
            .await
    }
}

pub(super) mod actions {
    use crate::entities::data;

    #[derive(Debug)]
    pub enum Action {
        Save(Save),
    }

    #[derive(Debug)]
    pub struct Save(pub data::ActiveModel);
}

pub(super) mod handlers {
    use sea_orm::DbErr;

    use crate::actors::Handler;

    use super::{actions::*, DataSaver, FLUSH_TIME};

    impl Handler<Save> for DataSaver {
        type Output = Result<(), DbErr>;

        async fn handle(&mut self, action: Save) -> Self::Output {
            log::trace!("action: {:?}", action);
            let Save(data) = action;
            self.buf.push(data);
            if self.buf.len() == FLUSH_TIME {
                let insert_result = self.mutation.bulk_insert_data(self.buf.drain(..)).await?;
                log::debug!("insert result: {:?}", insert_result);
            }

            Ok(())
        }
    }
}
