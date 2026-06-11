use std::sync::Arc;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

pub struct AcceptTimeoutCheckState(Notify);

impl AcceptTimeoutCheckState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self(Notify::new()))
    }

    pub fn signal(&self) {
        self.0.notify_one();
    }
}

// Wakes the state machine's select arm on a fixed interval so the leader can inspect
// each follower's in-flight batch queue without queuing behind other pending PaxosEvents.
pub struct AcceptTimeoutCheckWatcher(Arc<AcceptTimeoutCheckState>);

impl AcceptTimeoutCheckWatcher {
    pub fn new(state: Arc<AcceptTimeoutCheckState>) -> Self {
        Self(state)
    }

    pub async fn wait_for_check(&self, cancellation_token: &CancellationToken) {
        tokio::select! {
            biased;
            _ = cancellation_token.cancelled() => {}
            _ = self.0.0.notified() => {}
        }
    }
}
