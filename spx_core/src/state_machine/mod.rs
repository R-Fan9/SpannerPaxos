use crate::PaxosDispatcher;
use crate::roles::Follower;
use crate::{PaxosEvent, PaxosSharedContext};
use spx_lib::worker_runner::Worker;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tonic::async_trait;
use uuid::Uuid;

mod state;

pub use state::PaxosState;

pub struct PaxosStateMachine {
    event_rx: Mutex<Receiver<PaxosEvent>>,
    ctx: Arc<PaxosSharedContext>,
}

impl PaxosStateMachine {
    pub fn new(
        member_id: Uuid,
        dispatcher: Arc<dyn PaxosDispatcher>,
        event_rx: Receiver<PaxosEvent>,
    ) -> Self {
        let ctx = PaxosSharedContext::new(member_id, dispatcher);
        Self {
            event_rx: Mutex::new(event_rx),
            ctx: Arc::new(ctx),
        }
    }

    pub async fn start(
        &self,
        cancellation_token: CancellationToken,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Start as a follower
        let mut current_state = PaxosState::Follower(Follower::new());

        // Start the Paxos state machine loop
        let mut event_rx = self.event_rx.lock().await;
        loop {
            tokio::select! {
                // This ensures the cancellation branch is checked first if multiple branches are ready
                biased;

                // If the cancellation token is triggered, break out of the loop
                _ = cancellation_token.cancelled() => {
                    break;
                }

                // Continuously wait for leader lease expiration
                _ = self.ctx.leader_lease_expired() => {
                        current_state = current_state.process_event(PaxosEvent::LeaderLeaseExpired, self.ctx.clone())
                        .await
                        .expect("Failed to process leader lease expiration");
                }

                // Listen to the next incoming Paxos event from the channel
                maybe_event = event_rx.recv() => {
                    match maybe_event{
                        Some(event) => {
                            current_state = current_state
                            .process_event(event, self.ctx.clone())
                            .await
                            .expect("Failed to process Paxos event");
                        }

                        // No Paxos event received, indicating the event channel might have been closed
                        None => {
                            println!("Paxos event channel closed");
                            break;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Worker for PaxosStateMachine {
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
        cancellation_token: CancellationToken,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        todo!()
    }
}
