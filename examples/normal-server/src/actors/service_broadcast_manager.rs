use anyhow::Ok;
use smallvec::SmallVec;
use tokio::join;

use crate::actors::Handler;

use self::actions::{RegisterConnection, UnregisterConnection};

use super::service_broadcaster::{self, LaunchedServiceBroadcaster, ServiceBroadcaster};

#[derive(Debug)]
pub struct ServiceBroadcastManagerBuilder {
    service_broadcaster: ServiceBroadcaster,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the error originated from the manager")]
    Manager(#[source] anyhow::Error),
    #[error("the error originated from the broadcaster")]
    Broadcaster(#[source] service_broadcaster::ProcessActionError),
}

impl ServiceBroadcastManagerBuilder {
    pub fn launch(
        self,
    ) -> (
        LaunchedServiceBroadcastManager,
        tokio::task::JoinHandle<SmallVec<[Error; 2]>>,
    ) {
        let (launched_broadcaster, broadcaster_handle) = self.service_broadcaster.launch();

        let manager = ServiceBroadcastManager {
            activated_connections: 0,
            launched_broadcaster: launched_broadcaster,
        };

        let (launched_manager, manager_handle) = manager.launch();

        let join_handler = actix_rt::spawn(async move {
            let (broadcaster_result, manager_result) = join!(broadcaster_handle, manager_handle);
            let (broadcast_result, manager_result) = (
                broadcaster_result.expect("the service broadcaster panicked"),
                manager_result.expect("the service broadcast manager panicked"),
            );

            let mut result = SmallVec::<[Error; 2]>::new();
            if let Err(broadcast_err) = broadcast_result {
                result.push(Error::Broadcaster(broadcast_err));
            }
            if let Err(manager_err) = manager_result {
                result.push(Error::Manager(manager_err));
            }

            result
        });

        (launched_manager, join_handler)
    }
}

#[derive(Debug)]
pub struct ServiceBroadcastManager {
    activated_connections: i32,
    launched_broadcaster: LaunchedServiceBroadcaster,
}

impl ServiceBroadcastManager {
    pub fn new(service_broadcaster: ServiceBroadcaster) -> ServiceBroadcastManagerBuilder {
        ServiceBroadcastManagerBuilder {
            service_broadcaster: service_broadcaster,
        }
    }

    fn launch(
        self,
    ) -> (
        LaunchedServiceBroadcastManager,
        tokio::task::JoinHandle<anyhow::Result<()>>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::channel::<actions::Action>(4);
        let join_handle = actix_rt::spawn(async move { self.task(rx).await });

        (LaunchedServiceBroadcastManager { tx }, join_handle)
    }

    async fn task(
        mut self,
        mut rx: tokio::sync::mpsc::Receiver<actions::Action>,
    ) -> anyhow::Result<()> {
        while let Some(action) = rx.recv().await {
            self.process_action(action).await?;
        }

        Ok(())
    }

    async fn process_action(&mut self, action: actions::Action) -> anyhow::Result<()> {
        log::debug!("{:?}", action);

        match action {
            actions::Action::RegisterConnection(act) => self.handle(act).await,
            actions::Action::UnregisterConnection(act) => self.handle(act).await,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LaunchedServiceBroadcastManager {
    tx: tokio::sync::mpsc::Sender<actions::Action>,
}

impl LaunchedServiceBroadcastManager {
    pub async fn register_connection(
        &self,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<actions::Action>> {
        self.tx
            .send(actions::Action::RegisterConnection(RegisterConnection))
            .await
    }

    pub async fn unregister_connection(
        &self,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<actions::Action>> {
        self.tx
            .send(actions::Action::UnregisterConnection(UnregisterConnection))
            .await
    }

    pub fn try_unregister_connection(
        &self,
    ) -> Result<(), tokio::sync::mpsc::error::TrySendError<actions::Action>> {
        self.tx
            .try_send(actions::Action::UnregisterConnection(UnregisterConnection))
    }
}

pub(super) mod actions {
    #[derive(Debug)]
    pub enum Action {
        RegisterConnection(RegisterConnection),
        UnregisterConnection(UnregisterConnection),
    }

    #[derive(Debug)]
    pub struct RegisterConnection;

    #[derive(Debug)]
    pub struct UnregisterConnection;
}

pub(super) mod handlers {
    use anyhow::anyhow;

    use crate::actors::Handler;

    use super::{actions::*, ServiceBroadcastManager};

    impl Handler<RegisterConnection> for ServiceBroadcastManager {
        type Output = anyhow::Result<()>;

        async fn handle(&mut self, _action: RegisterConnection) -> Self::Output {
            use crate::actors::service_broadcaster::Status;

            let broadcaster_should_on = self.activated_connections == 0;
            self.activated_connections += 1;
            if broadcaster_should_on {
                self.launched_broadcaster
                    .set_status(Status::Freezed)
                    .await?;
            }

            Ok(())
        }
    }

    impl Handler<UnregisterConnection> for ServiceBroadcastManager {
        type Output = anyhow::Result<()>;

        async fn handle(&mut self, _action: UnregisterConnection) -> Self::Output {
            use crate::actors::service_broadcaster::Status;

            if self.activated_connections <= 0 {
                return Err(
                    anyhow!("there is no activated connection left, but now the action requests to decrease it")
                );
            }

            self.activated_connections -= 1;

            if self.activated_connections <= 0 {
                self.launched_broadcaster
                    .set_status(Status::Activated)
                    .await?;
            }

            log::debug!(current_activated_connections = self.activated_connections; "Unregistered connection.");

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::{
        actors::service_broadcaster::test_utils::{
            log_socket_service_broadcast, setup_broadcaster_from_settings,
        },
        tests_utils::{settings, setup_logger},
    };

    use super::ServiceBroadcastManager;

    #[ignore]
    #[actix_rt::test]
    async fn it_works() {
        setup_logger();
        let settings = settings();
        let broadcaster = setup_broadcaster_from_settings(settings.clone()).await;
        let manager = ServiceBroadcastManager::new(broadcaster);
        let (launched_manager, _manager_join_handle) = manager.launch();

        let log_socket_handle =
            actix_rt::spawn(
                async move { log_socket_service_broadcast(settings.broadcast.port).await },
            );

        tokio::time::sleep(Duration::from_millis(3000)).await;
        launched_manager.register_connection().await.unwrap();
        launched_manager.register_connection().await.unwrap();
        tokio::time::sleep(Duration::from_millis(3000)).await;
        launched_manager.unregister_connection().await.unwrap();
        launched_manager.unregister_connection().await.unwrap();

        log_socket_handle.await.unwrap();
    }
}

impl Drop for ServiceBroadcastManager {
    fn drop(&mut self) {
        log::debug!(current_activated_connections = self.activated_connections; "Dropping the service broadcast manager")
    }
}
