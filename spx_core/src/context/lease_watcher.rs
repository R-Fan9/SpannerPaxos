use chrono::{DateTime, Utc};
use spx_lib::true_time::TrueTime;
use std::sync::Arc;
use tokio::sync::{Notify, RwLock};
use tokio_util::sync::CancellationToken;

pub struct LeaseState {
    pub(super) expiry: RwLock<Option<DateTime<Utc>>>,
    pub(super) notify: Notify,
}

impl LeaseState {
    pub fn new_expired() -> Arc<Self> {
        Arc::new(Self {
            // Initialize to an already-expired time so LeaderLeaseExpired fires immediately
            // on startup before any lease has been granted.
            expiry: RwLock::new(Some(DateTime::<Utc>::UNIX_EPOCH)),
            notify: Notify::new(),
        })
    }
}

// Polls the lease expiry without borrowing PaxosSharedContext, so &mut ctx can be
// passed into event handlers inside the state machine's tokio::select!.
pub struct LeaseWatcher(pub(super) Arc<LeaseState>);

impl LeaseWatcher {
    pub async fn wait_until_expired(&self, cancellation_token: &CancellationToken) {
        loop {
            let lease = *self.0.expiry.read().await;
            match lease {
                None => {
                    tokio::select! {
                        biased;
                        _ = cancellation_token.cancelled() => return,
                        _ = self.0.notify.notified() => continue,
                    }
                }
                Some(expiry) => {
                    tokio::select! {
                        biased;
                        _ = cancellation_token.cancelled() => return,
                        _ = self.0.notify.notified() => continue,
                        _ = TrueTime::commit_wait(expiry) => {
                            *self.0.expiry.write().await = None;
                            return;
                        }
                    }
                }
            }
        }
    }
}
