use std::sync::Arc;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

pub struct HeartbeatState(pub Notify);

impl HeartbeatState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self(Notify::new()))
    }
}

// Wakes the state machine's select arm when the heartbeat countdown expires,
// signalling the leader to broadcast a heartbeat accept request to advance t_safe on followers.
pub struct HeartbeatWatcher(pub(super) Arc<HeartbeatState>);

impl HeartbeatWatcher {
    pub async fn wait_for_heartbeat(&self, cancellation_token: &CancellationToken) {
        tokio::select! {
            biased;
            _ = cancellation_token.cancelled() => {}
            _ = self.0.0.notified() => {}
        }
    }
}
