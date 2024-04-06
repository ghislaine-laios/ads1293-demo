use actix_rt::time::Instant;
use actix_rt::time::Interval;
use std::time::Duration;

pub struct WatchDog {
    timeout: Duration,
    last_alive: Instant,
    ticker: Interval,
}

pub struct Timeout {
    timeout: Duration,
    last_alive: Instant,
    now: actix_rt::time::Instant,
}

impl WatchDog {
    pub fn new(timeout: Duration, check_interval: Duration) -> Self {
        Self {
            timeout,
            last_alive: Instant::now(),
            ticker: actix_rt::time::interval(check_interval),
        }
    }

    pub async fn tick(&mut self) -> (bool, Instant) {
        let now = self.ticker.tick().await;
        let duration = now.duration_since(self.last_alive.clone());
        log::trace!("tick after duration: {:?}", duration);
        (duration <= self.timeout, now)
    }

    pub async fn wait_until_timeout(&mut self) -> Timeout {
        loop {
            let (alive, now) = self.tick().await;
            if !alive {
                break Timeout {
                    timeout: self.timeout,
                    last_alive: self.last_alive,
                    now,
                };
            }
        }
    }

    pub fn notify_alive(&mut self) {
        self.last_alive = actix_rt::time::Instant::now()
    }
}
