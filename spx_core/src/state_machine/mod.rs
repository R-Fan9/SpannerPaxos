use crate::PaxosDispatcher;
use crate::PaxosEvent;
use crate::context::PaxosSharedContext;
use crate::roles::Follower;
use spx_lib::worker_runner::Worker;
use std::collections::HashSet;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tonic::async_trait;
use uuid::Uuid;

mod state;

pub use state::PaxosState;

pub struct PaxosStateMachine {
    member_id: Uuid,
    peer_ids: HashSet<Uuid>,
    dispatcher: Arc<dyn PaxosDispatcher>,
    event_tx: Sender<PaxosEvent>,
    event_rx: Mutex<Receiver<PaxosEvent>>,
}

impl PaxosStateMachine {
    pub fn new(
        member_id: Uuid,
        peer_ids: HashSet<Uuid>,
        dispatcher: Arc<dyn PaxosDispatcher>,
        event_tx: Sender<PaxosEvent>,
        event_rx: Receiver<PaxosEvent>,
    ) -> Self {
        Self {
            member_id,
            peer_ids,
            dispatcher,
            event_tx,
            event_rx: Mutex::new(event_rx),
        }
    }


    pub async fn start(
        &self,
        cancellation_token: CancellationToken,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut ctx = PaxosSharedContext::new(
            self.member_id,
            self.peer_ids.clone(),
            self.dispatcher.clone(),
            self.event_tx.clone(),
            cancellation_token.clone(),
        );
        let lease_watcher = ctx.lease_watcher();
        let write_flush_watcher = ctx.write_flush_watcher();
        let accept_timeout_check_watcher = ctx.accept_timeout_check_watcher();
        let mut current_state = PaxosState::Follower(Follower::new(None));
        let mut event_rx = self.event_rx.lock().await;

        loop {
            tokio::select! {
                // This ensures the cancellation branch is checked first if multiple branches are ready
                biased;

                // Continuously wait for leader lease expiration; returns early on cancellation
                _ = lease_watcher.wait_until_expired(&cancellation_token) => {
                    if cancellation_token.is_cancelled() {
                        break;
                    }
                    current_state = current_state
                        .process_event(PaxosEvent::LeaderLeaseExpired, &mut ctx)
                        .await
                        .expect("Failed to process leader lease expiration");
                }

                // Wake on a fixed interval to let the leader check for timed-out in-flight
                // batches, bypassing the event queue; returns early on cancellation
                _ = accept_timeout_check_watcher.wait_for_check(&cancellation_token) => {
                    if cancellation_token.is_cancelled() {
                        break;
                    }
                    current_state = current_state
                        .process_event(PaxosEvent::AcceptTimeoutCheckFired, &mut ctx)
                        .await
                        .expect("Failed to process accept timeout check");
                }

                // Wake immediately when the leader signals a write flush (batch full or timer
                // fired), bypassing the event queue so the dispatch is not delayed by other events;
                // returns early on cancellation
                _ = write_flush_watcher.wait_for_flush(&cancellation_token) => {
                    if cancellation_token.is_cancelled() {
                        break;
                    }
                    current_state = current_state
                        .process_event(PaxosEvent::WriteFlushTimerFired, &mut ctx)
                        .await
                        .expect("Failed to process write flush");
                }

                // Listen to the next incoming Paxos event from the channel
                maybe_event = event_rx.recv() => {
                    match maybe_event {
                        Some(event) => {
                            current_state = current_state
                                .process_event(event, &mut ctx)
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
        _cancellation_token: CancellationToken,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        todo!()
    }
}
