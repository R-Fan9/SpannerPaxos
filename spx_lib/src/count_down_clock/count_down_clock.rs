use rand::Rng;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time;
use tokio_util::sync::CancellationToken;

const MIN_RANDOM_WAIT_MS: u64 = 30;

/// A count-down clock that fires a callback when the countdown expires.
/// Supports reset (restart the countdown) via `reset()` and cancellation via `cancel()`.
/// Only one background task runs at a time; subsequent `start_*` calls are no-ops while running.
pub struct CountDownClock {
    reset_notify: Arc<Notify>,
    on_expire: Arc<dyn Fn() + Send + Sync + 'static>,
    token: CancellationToken,
    is_running: Arc<AtomicBool>,
}

impl CountDownClock {
    pub fn new(on_expire: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            reset_notify: Arc::new(Notify::new()),
            on_expire: Arc::new(on_expire),
            token: CancellationToken::new(),
            is_running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Signals the running countdown to discard the current wait and restart.
    pub fn reset(&self) {
        self.reset_notify.notify_one();
    }

    /// Stops the running countdown task.
    pub fn cancel(&self) {
        self.token.cancel();
    }

    /// Spawns a background task that waits a random duration between 30ms and `max_ms`,
    /// then fires the callback. No-op if a task is already running.
    pub fn start_random(&self, max_ms: u64) {
        assert!(
            max_ms > MIN_RANDOM_WAIT_MS,
            "max_ms must be greater than {MIN_RANDOM_WAIT_MS}ms, got {max_ms}"
        );
        if self.is_running.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            return;
        }
        let reset_notify = self.reset_notify.clone();
        let on_expire = self.on_expire.clone();
        let token = self.token.clone();
        let is_running = self.is_running.clone();
        tokio::spawn(async move {
            loop {
                let wait_ms = rand::thread_rng().gen_range(MIN_RANDOM_WAIT_MS..=max_ms);
                tokio::select! {
                    biased;
                    _ = token.cancelled() => break,
                    _ = reset_notify.notified() => continue,
                    _ = time::sleep(Duration::from_millis(wait_ms)) => {
                        on_expire();
                        break;
                    }
                }
            }
            is_running.store(false, Ordering::Release);
        });
    }

    /// Spawns a background task that waits exactly `ms`, then fires the callback.
    /// No-op if a task is already running.
    pub fn start_fixed(&self, ms: u64) {
        if self.is_running.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            return;
        }
        let reset_notify = self.reset_notify.clone();
        let on_expire = self.on_expire.clone();
        let token = self.token.clone();
        let is_running = self.is_running.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => break,
                    _ = reset_notify.notified() => continue,
                    _ = time::sleep(Duration::from_millis(ms)) => {
                        on_expire();
                        break;
                    }
                }
            }
            is_running.store(false, Ordering::Release);
        });
    }
}
