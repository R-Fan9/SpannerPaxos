use std::sync::Arc;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

pub struct WriteFlushState(pub(super) Notify);

impl WriteFlushState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self(Notify::new()))
    }
}

// Wakes the state machine's select arm immediately when the leader signals a flush
// (batch-size threshold reached or periodic timer fired), bypassing the event queue.
pub struct WriteFlushWatcher(pub(super) Arc<WriteFlushState>);

impl WriteFlushWatcher {
    pub async fn wait_for_flush(&self, cancellation_token: &CancellationToken) {
        tokio::select! {
            biased;
            _ = cancellation_token.cancelled() => {}
            _ = self.0.0.notified() => {}
        }
    }
}
