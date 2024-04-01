use crate::settings::{self, BroadcastInfo};
use anyhow::Context;
use async_std::net::UdpSocket;
use serde::{Deserialize, Serialize};
use tokio::select;

use self::{actions::Action, handlers::Handler, interval_handlers::ServiceBroadcast};

const SERVICE_NAME: &'static str = "ADS1293-DEMO-NORMAL-SERVICE";

pub enum Status {
    Activated,
    Freezed,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Message {
    pub service: String,
    pub bind_to: settings::BindTo,
}

pub struct ServiceBroadcaster {
    status: Status,
    socket: UdpSocket,
    broadcast_info: BroadcastInfo,
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

    pub fn launch(self) -> LaunchedServiceBroadcaster {
        use tokio::sync::mpsc;

        let (tx, rx) = mpsc::channel::<Action>(16);

        let join_handle = actix_rt::spawn(async move { self.task(rx).await });

        LaunchedServiceBroadcaster { tx, join_handle }
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

pub struct LaunchedServiceBroadcaster {
    pub tx: tokio::sync::mpsc::Sender<actions::Action>,
    pub join_handle: tokio::task::JoinHandle<Result<(), ProcessActionError>>,
}

impl LaunchedServiceBroadcaster {}

pub mod actions {
    pub enum Action {
        SetStatus(SetStatus),
        Broadcast(Broadcast),
    }

    pub struct SetStatus(pub super::Status);

    pub struct Broadcast;
}

pub mod handlers {
    use super::{actions, ServiceBroadcaster};

    pub trait Handler<Action> {
        type Output;
        fn handle(
            &mut self,
            action: Action,
        ) -> impl std::future::Future<Output = Self::Output> + Send;
    }

    impl Handler<actions::SetStatus> for ServiceBroadcaster {
        type Output = ();
        async fn handle(&mut self, action: actions::SetStatus) {
            self.status = action.0;
        }
    }

    impl Handler<actions::Broadcast> for ServiceBroadcaster {
        type Output = anyhow::Result<()>;

        async fn handle(&mut self, _action: actions::Broadcast) -> anyhow::Result<()> {
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
mod tests {
    use async_std::net::UdpSocket;

    use crate::tests_utils::{settings, setup_logger};

    use super::{LaunchedServiceBroadcaster, Message, ServiceBroadcaster};

    #[actix_rt::test]
    async fn it_works() {
        setup_logger();
        let settings = settings();
        let broadcast_info = settings.broadcast.clone();

        let broadcaster = ServiceBroadcaster::new(settings.bind_to, settings.broadcast)
            .await
            .unwrap();
        let broadcaster = broadcaster.launch();

        let LaunchedServiceBroadcaster { tx, join_handle } = broadcaster;

        let join_handle_2 = actix_rt::spawn(async move {
            let socket = UdpSocket::bind(("0.0.0.0", broadcast_info.port))
                .await
                .unwrap();
            socket.set_broadcast(true).unwrap();
            let mut buf: [u8; 1024] = [0; 1024];
            for i in 1..10 {
                let (size, addr) = socket.recv_from(&mut buf).await.unwrap();
                let value: Message = serde_json::from_slice(&buf[..size]).unwrap();
                log::debug!("Index: {}", i);
                log::debug!("{:?}", addr);
                log::debug!("{:?}", value);
            }
        });

        let (r,) = tokio::join!(join_handle);
        r.unwrap().unwrap();

        join_handle_2.await.unwrap();
    }
}
