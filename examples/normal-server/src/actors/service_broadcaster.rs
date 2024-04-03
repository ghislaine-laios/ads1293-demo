use self::{actions::Action, interval_handlers::ServiceBroadcast};
use crate::{
    actors::Handler,
    settings::{self, BroadcastInfo},
};
use anyhow::Context;
use async_std::net::UdpSocket;
use serde::{Deserialize, Serialize};
use tokio::select;

const SERVICE_NAME: &'static str = "ADS1293-DEMO-NORMAL-SERVICE";

#[derive(Debug)]
pub enum Status {
    Activated,
    Freezed,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Message {
    pub service: String,
    pub bind_to: settings::BindTo,
}

#[derive(Debug)]
pub struct ServiceBroadcaster {
    status: Status,
    socket: UdpSocket,
    broadcast_info: BroadcastInfo,
    #[allow(dead_code)]
    message: Message,
    raw_message: Vec<u8>,
    service_broadcast: Option<interval_handlers::ServiceBroadcast>,
}

impl ServiceBroadcaster {
    pub async fn new(
        service_bind_to: settings::BindTo,
        broadcast_info: BroadcastInfo,
    ) -> anyhow::Result<Self> {
        let message = Message {
            service: SERVICE_NAME.to_owned(),
            bind_to: service_bind_to,
        };

        let raw_message = serde_json::to_vec(&message)?;

        let bind_to = (message.bind_to.ip.as_str(), message.bind_to.port);
        let socket = UdpSocket::bind(bind_to).await?;
        socket.set_broadcast(true)?;

        Ok(Self {
            status: Status::Activated,
            socket,
            broadcast_info,
            message,
            raw_message,
            service_broadcast: Some(ServiceBroadcast::new()),
        })
    }

    pub fn launch(
        self,
    ) -> (
        LaunchedServiceBroadcaster,
        tokio::task::JoinHandle<Result<(), ProcessActionError>>,
    ) {
        use tokio::sync::mpsc;

        let (tx, rx) = mpsc::channel::<Action>(16);

        let join_handle = actix_rt::spawn(async move { self.task(rx).await });

        (LaunchedServiceBroadcaster { tx }, join_handle)
    }

    async fn task(
        mut self,
        mut rx: tokio::sync::mpsc::Receiver<Action>,
    ) -> Result<(), ProcessActionError> {
        use tokio::select;

        let (interval_tx, mut interval_rx) = tokio::sync::mpsc::channel::<Action>(1);
        let mut service_broadcast = self.service_broadcast.take().unwrap();

        loop {
            select! {
                action = rx.recv() => {
                    match self.process_action(action).await {
                        Ok(_) => continue,
                        Err(ProcessActionError::None) => break,
                        Err(e) => return Err(e)

                    }
                },
                action = interval_rx.recv() => {
                    match self.process_action(action).await {
                        Ok(_)  => continue,
                        Err(ProcessActionError::None) =>
                        return Err(ProcessActionError::InternalBug(anyhow::anyhow!(""))),
                        Err(e) => return Err(e)
                    }
                },
                action = Self::interval_task(&mut service_broadcast) => {
                    interval_tx.send(action).await.context("this is a bug")?;
                }
            }
        }

        Ok(())
    }

    async fn interval_task(service_broadcast: &mut ServiceBroadcast) -> actions::Action {
        service_broadcast.tick().await
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ProcessActionError {
    #[error("the action is none")]
    None,
    #[error("failed to process the action")]
    HandlingError(#[from] anyhow::Error),
    #[error("a bug occurred")]
    InternalBug(#[source] anyhow::Error),
}

impl ServiceBroadcaster {
    async fn process_action(
        &mut self,
        action: Option<actions::Action>,
    ) -> Result<(), ProcessActionError> {
        use ProcessActionError as Error;

        log::debug!("{:?}", &action);

        let Some(action) = action else {
            return Err(Error::None);
        };

        match action {
            actions::Action::SetStatus(action) => {
                self.handle(action).await;
            }
            actions::Action::Broadcast(action) => {
                self.handle(action).await?;
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct LaunchedServiceBroadcaster {
    pub tx: tokio::sync::mpsc::Sender<actions::Action>,
}

impl LaunchedServiceBroadcaster {
    pub async fn set_status(&self, status: Status) -> anyhow::Result<()> {
        self.tx
            .send(actions::Action::SetStatus(actions::SetStatus(status)))
            .await?;

        Ok(())
    }
}

pub(super) mod actions {
    #[derive(Debug)]
    pub enum Action {
        SetStatus(SetStatus),
        Broadcast(Broadcast),
    }

    #[derive(Debug)]
    pub struct SetStatus(pub super::Status);

    #[derive(Debug)]
    pub struct Broadcast;
}

pub(super) mod handlers {
    use crate::actors::Handler;

    use super::{actions, ServiceBroadcaster, Status};

    impl Handler<actions::SetStatus> for ServiceBroadcaster {
        type Output = ();
        async fn handle(&mut self, action: actions::SetStatus) {
            self.status = action.0;
        }
    }

    impl Handler<actions::Broadcast> for ServiceBroadcaster {
        type Output = anyhow::Result<()>;

        async fn handle(&mut self, _action: actions::Broadcast) -> anyhow::Result<()> {
            if !matches!(self.status, Status::Activated) {
                log::debug!("{:?}", self.status);
                return Ok(());
            }

            self.socket
                .send_to(
                    &self.raw_message,
                    (self.broadcast_info.ip.as_str(), self.broadcast_info.port),
                )
                .await?;
            Ok(())
        }
    }
}

pub(super) mod interval_handlers {
    use std::time::Duration;

    use super::actions::{Action, Broadcast};

    #[derive(Debug)]
    pub struct ServiceBroadcast(tokio::time::Interval, tokio::time::Instant);

    impl ServiceBroadcast {
        pub fn new() -> Self {
            Self(
                tokio::time::interval(Duration::from_secs(1)),
                tokio::time::Instant::now(),
            )
        }

        pub async fn tick(&mut self) -> Action {
            let instant = self.0.tick().await;
            let last_instant = self.1.clone();
            let duration = instant.duration_since(last_instant);
            log::trace!("tick after duration: {:#?}", duration);
            self.1 = instant;
            Action::Broadcast(Broadcast)
        }
    }
}

#[cfg(test)]
pub(crate) mod test_utils {
    use async_std::net::UdpSocket;

    use crate::settings::Settings;

    use super::ServiceBroadcaster;

    pub async fn setup_broadcaster_from_settings(settings: Settings) -> ServiceBroadcaster {
        ServiceBroadcaster::new(settings.bind_to, settings.broadcast)
            .await
            .unwrap()
    }

    pub(crate) async fn log_socket_service_broadcast(port: u16) {
        let socket = UdpSocket::bind(("0.0.0.0", port)).await.unwrap();
        socket.set_broadcast(true).unwrap();
        let mut buf: [u8; 1024] = [0; 1024];
        for i in 1..10 {
            let (size, addr) = socket.recv_from(&mut buf).await.unwrap();
            let value: super::Message = serde_json::from_slice(&buf[..size]).unwrap();
            log::debug!("Index: {}", i);
            log::debug!("{:?}", addr);
            log::debug!("{:?}", value);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::tests_utils::{settings, setup_logger};

    use super::test_utils::{log_socket_service_broadcast, setup_broadcaster_from_settings};

    #[ignore]
    #[actix_rt::test]
    async fn it_works() {
        setup_logger();
        let settings = settings();
        let broadcast_info = settings.broadcast.clone();

        let broadcaster = setup_broadcaster_from_settings(settings.clone()).await;
        let (broadcaster, _join_handle) = broadcaster.launch();

        let join_handle_2 =
            actix_rt::spawn(async move { log_socket_service_broadcast(broadcast_info.port).await });

        let join_handle_3 = actix_rt::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            broadcaster
                .set_status(super::Status::Freezed)
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_secs(3)).await;
            broadcaster
                .set_status(super::Status::Activated)
                .await
                .unwrap();

            // used to prevent dropping the broadcaster since we move it into this closure
            tokio::time::sleep(Duration::from_secs(5)).await;
        });

        // let (r,) = tokio::join!(join_handle);
        // r.unwrap().unwrap();

        join_handle_3.await.unwrap();
        join_handle_2.await.unwrap();
    }
}
