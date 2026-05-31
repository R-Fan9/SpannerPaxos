use crate::models::LogEntry;
use crate::{PaxosDispatcher, PaxosEvent, PreVoteRequest, VoteRequest};
use chrono::{DateTime, Duration, Utc};
use spx_lib::true_time::TrueTime;
use spx_lib::write_ahead_log::WriteAheadLog;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio::sync::{Notify, RwLock};
use uuid::Uuid;

// Holds the shared lease state between PaxosSharedContext and the LeaseWatcher.
// Wrapped in Arc so both sides can cheaply clone a handle without borrowing ctx.
struct LeaseState {
    expiry: RwLock<Option<DateTime<Utc>>>,
    notify: Notify,
}

// A lightweight handle used in tokio::select! to wait for lease expiration without
// borrowing PaxosSharedContext, allowing &mut ctx to be passed into event handlers.
pub struct LeaseWatcher(Arc<LeaseState>);

impl LeaseWatcher {
    pub async fn wait_until_expired(&self) {
        loop {
            let lease = *self.0.expiry.read().await;
            match lease {
                None => {
                    self.0.notify.notified().await;
                }
                Some(expiry) => {
                    tokio::select! {
                        biased;
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

pub struct PaxosSharedContext {
    // The unique identifier for this member (node)
    member_id: Uuid,

    // The unique identifiers of the other members (nodes) in the Paxos group
    peer_ids: HashSet<Uuid>,

    // The term (ballot) number that this member (node) is currently in
    term: u32,

    // The time at which this member (node) is safe to serve a read request
    t_safe: Option<DateTime<Utc>>,

    // The local expiry time of the leader lease and its update notification channel.
    // Wrapped in Arc so a LeaseWatcher can hold a handle without borrowing ctx,
    // allowing &mut ctx to be passed into event handlers inside tokio::select!.
    // Each node computes the expiry independently of its own clock rather than sharing
    // a single timestamp across nodes — clock skew means the same wall-clock timestamp
    // can fall in the past on one node while still in the future on another, so a shared
    // expiry would break the safety guarantee that only one leader holds the lease at a time.
    lease: Arc<LeaseState>,

    // The lease duration used for Paxos leases, default to 10 seconds
    lease_length: Duration,

    // The dispatcher for dispatching Paxos requests to other Paxos members
    dispatcher: Arc<dyn PaxosDispatcher>,

    // The sender for posting Paxos events back into the event loop internally
    event_tx: Sender<PaxosEvent>,

    // The term number of the last log entry persisted by this member (node)
    last_log_term: u32,

    // The slot number of the last log entry persisted by this member (node)
    last_log_slot: u32,

    // The slot number of the last log entry committed by this member (node)
    committed_slot: u32,

    // The log entries that have been persisted but not yet committed by this member (node)
    uncommitted_logs: BTreeMap<u32, LogEntry>,

    // The write-ahead log service for persisting log entries
    wal: WriteAheadLog,
}

impl PaxosSharedContext {
    pub fn new(
        member_id: Uuid,
        peer_ids: HashSet<Uuid>,
        dispatcher: Arc<dyn PaxosDispatcher>,
        event_tx: Sender<PaxosEvent>,
    ) -> Self {
        Self {
            member_id,
            peer_ids,
            term: 0,
            t_safe: None,
            // Initialize to an already-expired time so LeaderLeaseExpired fires immediately on startup
            lease: Arc::new(LeaseState {
                expiry: RwLock::new(Some(DateTime::<Utc>::UNIX_EPOCH)),
                notify: Notify::new(),
            }),
            last_log_term: 0,
            last_log_slot: 0,
            committed_slot: 0,
            uncommitted_logs: BTreeMap::new(),
            lease_length: Duration::seconds(10),
            dispatcher,
            event_tx,
            wal: WriteAheadLog::new(),
        }
    }

    pub fn lease_watcher(&self) -> LeaseWatcher {
        LeaseWatcher(Arc::clone(&self.lease))
    }

    pub fn get_event_sender(&self) -> Sender<PaxosEvent> {
        self.event_tx.clone()
    }

    pub fn get_dispatcher(&self) -> Arc<dyn PaxosDispatcher> {
        self.dispatcher.clone()
    }

    pub fn get_current_term(&self) -> u32 {
        self.term
    }

    pub fn get_next_term(&self) -> u32 {
        self.term + 1
    }

    pub fn set_current_term(&mut self, term: u32) {
        self.term = term;
    }

    pub fn increment_current_term(&mut self) {
        self.term += 1;
    }

    pub fn get_current_member_id(&self) -> Uuid {
        self.member_id
    }

    pub fn log_prefix(&self, role: &str) -> String {
        format!("[{} {}, term {}]", role, self.member_id, self.term)
    }

    pub fn get_peer_ids(&self) -> &HashSet<Uuid> {
        &self.peer_ids
    }

    pub fn get_last_log_term(&self) -> u32 {
        self.last_log_term
    }

    pub fn get_last_log_slot(&self) -> u32 {
        self.last_log_slot
    }

    pub fn get_committed_slot(&self) -> u32 {
        self.committed_slot
    }

    pub fn set_committed_slot(&mut self, slot: u32) {
        self.committed_slot = slot;
    }

    pub fn set_last_log_slot(&mut self, slot: u32) {
        self.last_log_slot = slot;
    }

    pub fn set_last_log_term(&mut self, term: u32) {
        self.last_log_term = term;
    }

    pub fn get_uncommitted_logs(&self) -> &BTreeMap<u32, LogEntry> {
        &self.uncommitted_logs
    }

    pub fn get_uncommitted_logs_mut(&mut self) -> &mut BTreeMap<u32, LogEntry> {
        &mut self.uncommitted_logs
    }

    pub fn get_lease_length(&self) -> Duration {
        self.lease_length
    }

    pub async fn update_leader_lease_expiry_time(&self, expiry: DateTime<Utc>) {
        *self.lease.expiry.write().await = Some(expiry);
        self.lease.notify.notify_one();
    }

    pub async fn is_leader_lease_expired(&self) -> bool {
        let Some(expiry) = *self.lease.expiry.read().await else {
            return true;
        };
        TrueTime::after(expiry)
    }

    pub fn create_pre_vote(&self) -> PreVoteRequest {
        PreVoteRequest {
            member_id: self.member_id,
            next_term: self.term + 1,
            last_log_term: self.last_log_term,
            last_log_slot: self.last_log_slot,
        }
    }

    pub fn create_vote(&self) -> VoteRequest {
        VoteRequest {
            member_id: self.member_id,
            term: self.term,
            last_log_term: self.last_log_term,
            last_log_slot: self.last_log_slot,
        }
    }

    pub fn get_wal(&self) -> &WriteAheadLog {
        &self.wal
    }

    pub fn get_wal_mut(&mut self) -> &mut WriteAheadLog {
        &mut self.wal
    }
}
