use actix_rt::time::Instant;
use actix_rt::time::Interval;
use futures::Future;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::actors::Handler;
use crate::errors::ChannelClosedError;
use actions::NotifyAlive;

#[derive(Debug)]
pub struct WatchDog {
    timeout: Duration,
    last_alive: Instant,
    ticker: Interval,
}

#[allow(unused)]
pub struct Timeout {
    timeout: Duration,
    last_alive: Instant,
    now: actix_rt::time::Instant,
}

impl WatchDog {
    pub fn new(timeout: Duration, check_interval: Duration) -> Self {
        log::debug!(
            "new watch dog: timeout: {:?}, check_interval: {:?}",
            timeout,
            check_interval
        );
        Self {
            timeout,
            last_alive: Instant::now(),
            ticker: actix_rt::time::interval(check_interval),
        }
    }

    pub async fn tick(&mut self) -> (bool, Instant) {
        log::trace!("tick begin.");
        let now = self.ticker.tick().await;
        let duration = now.duration_since(self.last_alive.clone());
        log::trace!("tick after duration: {:?}", duration);
        (duration <= self.timeout, now)
    }

    pub fn launch(self) -> (LaunchedWatchDog, TimeoutHandle) {
        let (launched, action_receiver) = LaunchedWatchDog::new();
        (
            launched,
            actix_rt::spawn(self.wait_until_timeout(action_receiver)),
        )
    }

    pub fn launch_inline(self) -> (LaunchedWatchDog, impl Future<Output = Option<Timeout>>) {
        let (launched, action_receiver) = LaunchedWatchDog::new();
        (launched, self.wait_until_timeout(action_receiver))
    }

    async fn wait_until_timeout(mut self, mut action_receiver: ActionReceiver) -> Option<Timeout> {
        let result = loop {
            log::trace!("watch dog loop begin.");
            tokio::select! {
                (alive, now) = self.tick() => {
                    if !alive {
                        break Some(Timeout {
                            timeout: self.timeout,
                            last_alive: self.last_alive,
                            now,
                        });
                    }
                },
                result = action_receiver.0.recv() => {
                    log::trace!("action: {:?}", result);
                    match result {
                        Some(_) => self.handle(NotifyAlive).await,
                        None => break None,
                    }
                }
            }
        };

        log::debug!("A watch dog has reached its timeout.");

        result
    }

    pub fn notify_alive(&mut self) {
        self.last_alive = actix_rt::time::Instant::now()
    }
}

pub struct ActionReceiver(mpsc::Receiver<actions::Action>);
pub struct LaunchedWatchDog(mpsc::Sender<actions::Action>);
pub type TimeoutHandle = actix_rt::task::JoinHandle<Option<Timeout>>;

impl LaunchedWatchDog {
    pub fn new() -> (LaunchedWatchDog, ActionReceiver) {
        let (tx, rx) = mpsc::channel::<actions::Action>(2);
        (LaunchedWatchDog(tx), ActionReceiver(rx))
    }

    pub async fn notify_alive(&self) -> Result<(), mpsc::error::SendTimeoutError<actions::Action>> {
        self.0
            .send_timeout(actions::Action::NotifyAlive, Duration::from_millis(10))
            .await
    }

    pub async fn do_notify_alive(&self) -> Result<(), ChannelClosedError> {
        if let Err(e) = self.0.try_send(actions::Action::NotifyAlive) {
            match e {
                mpsc::error::TrySendError::Full(_) => {}
                mpsc::error::TrySendError::Closed(_) => return Err(ChannelClosedError),
            }
        };

        Ok(())
    }
}

pub(super) mod actions {
    #[derive(Debug)]
    pub enum Action {
        NotifyAlive,
    }

    #[derive(Debug, Default)]
    pub struct NotifyAlive;
}

pub(super) mod handlers {
    use crate::actors::Handler;

    use super::{actions::NotifyAlive, WatchDog};

    impl Handler<NotifyAlive> for WatchDog {
        type Output = ();

        async fn handle(&mut self, _action: NotifyAlive) -> Self::Output {
            self.notify_alive()
        }
    }
}
