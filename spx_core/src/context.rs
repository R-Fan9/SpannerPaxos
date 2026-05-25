use crate::models::LogEntry;
use crate::{PaxosDispatcher, PaxosEvent, PreVoteRequest, VoteRequest};
use chrono::{DateTime, Duration, Utc};
use dashmap::DashSet;
use spx_lib::true_time::TrueTime;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::mpsc::Sender;
use tokio::sync::{Notify, RwLock};
use uuid::Uuid;

// A thread-safe struct that contains shared context across all states of a Paxos group member
pub struct PaxosSharedContext {
    // The unique identifier for this member (node)
    member_id: Uuid,

    // The unique identifiers of the other members (nodes) in the Paxos group
    peer_ids: DashSet<Uuid>,

    // The term (ballot) number that this member (node) is currently in
    term: AtomicU32,

    // The time at which this member (node) is safe to serve a read request
    t_safe: RwLock<Option<DateTime<Utc>>>,

    // The term number of the last log entry persisted by this member (node)
    last_log_term: AtomicU32,

    // The slot number of the last log entry persisted by this member (node)
    last_log_slot: AtomicU32,

    // The local expiry time of the leader lease. Each node computes this independently
    // of its own clock rather than sharing a single timestamp across nodes. Clocks on
    // different machines drift at different rates (clock skew), so the same wall-clock
    // timestamp can fall in the past on one node while still in the future on another,
    // using a shared expiry would cause nodes to disagree on whether the lease is still
    // valid, breaking the safety guarantee that only one leader holds the lease at a time
    leader_lease_expiry_time: RwLock<Option<DateTime<Utc>>>,

    // A notification channel to notify the update of the leader lease
    leader_lease_update_notify: Notify,

    // The log entries that have been persisted but not yet committed by this member (node)
    uncommitted_logs: RwLock<Vec<LogEntry>>,

    // The lease duration used for Paxos leases, default to 10 seconds
    lease_length: Duration,

    // The dispatcher for dispatching Paxos requests to other Paxos members
    dispatcher: Arc<dyn PaxosDispatcher>,

    // The sender for posting Paxos events back into the event loop internally
    event_tx: Sender<PaxosEvent>,
}

impl PaxosSharedContext {
    pub fn new(
        member_id: Uuid,
        peer_ids: DashSet<Uuid>,
        dispatcher: Arc<dyn PaxosDispatcher>,
        event_tx: Sender<PaxosEvent>,
    ) -> Self {
        Self {
            member_id,
            peer_ids,
            term: AtomicU32::new(0),
            t_safe: RwLock::new(None),
            last_log_term: AtomicU32::new(0),
            last_log_slot: AtomicU32::new(0),
            // Initialize to an already-expired time so LeaderLeaseExpired fires immediately on startup
            leader_lease_expiry_time: RwLock::new(Some(DateTime::<Utc>::UNIX_EPOCH)),
            leader_lease_update_notify: Notify::new(),
            uncommitted_logs: RwLock::new(Vec::new()),
            lease_length: Duration::seconds(10),
            dispatcher,
            event_tx,
        }
    }

    pub fn get_event_sender(&self) -> Sender<PaxosEvent> {
        self.event_tx.clone()
    }

    pub fn get_dispatcher(&self) -> Arc<dyn PaxosDispatcher> {
        self.dispatcher.clone()
    }

    pub fn get_current_term(&self) -> u32 {
        // Using Acquire ordering here to ensure the freshest value is read
        self.term.load(Ordering::Acquire)
    }

    pub fn get_next_term(&self) -> u32 {
        // Using Acquire ordering here to ensure the freshest value is read
        self.term.load(Ordering::Acquire) + 1
    }

    pub fn set_current_term(&self, term: u32) {
        // Using Release ordering here to ensure the new value is visible to other threads immediately
        self.term.store(term, Ordering::Release);
    }

    pub fn increment_current_term(&self) {
        // Using Release ordering here as the return value of fetch_add is not used
        // Only need to ensure the new value is immediately visible to other threads
        self.term.fetch_add(1, Ordering::Release);
    }

    pub fn get_current_member_id(&self) -> Uuid {
        self.member_id
    }

    pub fn log_prefix(&self, role: &str) -> String {
        format!(
            "[{} {}, term {}]",
            role,
            self.member_id,
            self.get_current_term()
        )
    }

    pub fn get_peer_ids(&self) -> &DashSet<Uuid> {
        &self.peer_ids
    }

    pub fn get_last_log_term(&self) -> u32 {
        self.last_log_term.load(Ordering::SeqCst)
    }
    pub fn get_last_log_slot(&self) -> u32 {
        self.last_log_slot.load(Ordering::SeqCst)
    }

    pub async fn get_uncommitted_entries(&self) -> Vec<LogEntry> {
        self.uncommitted_logs.read().await.clone()
    }

    pub fn get_lease_length(&self) -> Duration {
        self.lease_length
    }

    pub async fn update_leader_lease_expiry_time(&self, expiry: DateTime<Utc>) {
        let mut lease = self.leader_lease_expiry_time.write().await;
        *lease = Some(expiry);

        // Notify the leader lease expiration check task that the lease has been updated
        self.leader_lease_update_notify.notify_one();
    }

    pub async fn is_leader_lease_expired(&self) -> bool {
        let Some(expiry) = *self.leader_lease_expiry_time.read().await else {
            return true;
        };
        TrueTime::after(expiry)
    }

    pub async fn wait_until_leader_lease_expired(&self) {
        loop {
            let lease = *self.leader_lease_expiry_time.read().await;
            match lease {
                // No lease set; block until one is written
                None => {
                    self.leader_lease_update_notify.notified().await;
                }
                // Lease is set; wait for it to expire or for an update
                Some(expiry) => {
                    tokio::select! {
                        biased;
                        _ = self.leader_lease_update_notify.notified() => continue,
                        _ = TrueTime::commit_wait(expiry) => {
                            // Clear the lease so the next iteration blocks until a new one is set
                            *self.leader_lease_expiry_time.write().await = None;
                            return;
                        }
                    }
                }
            }
        }
    }

    pub fn create_pre_vote(&self) -> PreVoteRequest {
        PreVoteRequest {
            member_id: self.get_current_member_id(),
            next_term: self.get_next_term(),
            last_log_term: self.get_last_log_term(),
            last_log_slot: self.get_last_log_slot(),
        }
    }

    pub fn create_vote(&self) -> VoteRequest {
        VoteRequest {
            member_id: self.get_current_member_id(),
            term: self.get_current_term(),
            last_log_term: self.get_last_log_term(),
            last_log_slot: self.get_last_log_slot(),
        }
    }
}
