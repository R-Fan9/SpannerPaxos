use crate::worker_runner::Worker;
use async_trait::async_trait;
use rand::Rng;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time;
use tokio_util::sync::CancellationToken;

const MIN_WAIT_TIME_MS: u64 = 30;

/// A count-down clock that runs a background loop, invoking a callback when the countdown expires.
/// Supports reset (restart with a new random duration) via `reset()`.
/// Cancellation is handled by the `CancellationToken` passed to `on_start`.
pub struct CountDownClock {
    max_wait_time_ms: u64,
    reset_notify: Notify,
    on_expire: Arc<dyn Fn() + Send + Sync + 'static>,
}

impl CountDownClock {
    pub fn new(max_wait_time_ms: u64, on_expire: impl Fn() + Send + Sync + 'static) -> Self {
        assert!(
            max_wait_time_ms > MIN_WAIT_TIME_MS,
            "max_wait_time must be greater than {MIN_WAIT_TIME_MS}ms, got {max_wait_time_ms}"
        );
        Self {
            max_wait_time_ms,
            reset_notify: Notify::new(),
            on_expire: Arc::new(on_expire),
        }
    }

    /// Signals the background loop to discard the current wait and restart with a new random duration.
    pub fn reset(&self) {
        self.reset_notify.notify_one();
    }

    async fn start(
        &self,
        cancellation_token: CancellationToken,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        loop {
            let wait_ms = rand::thread_rng().gen_range(MIN_WAIT_TIME_MS..=self.max_wait_time_ms);

            tokio::select! {
                biased;

                _ = cancellation_token.cancelled() => break,

                _ = self.reset_notify.notified() => continue,

                _ = time::sleep(Duration::from_millis(wait_ms)) => {
                    (self.on_expire)();
                    break;
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Worker for CountDownClock {
    async fn on_start(
        self: Arc<Self>,
        cancellation_token: CancellationToken,
    ) -> Result<JoinHandle<Result<(), Box<dyn Error + Send + Sync>>>, Box<dyn Error + Send + Sync>>
    {
        let task = tokio::spawn(async move { self.start(cancellation_token).await });
        Ok(task)
    }

    async fn on_stop(
        &self,
        _cancellation_token: CancellationToken,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        Ok(())
    }
}
