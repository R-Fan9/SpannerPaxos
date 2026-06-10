use crate::models::LogEntry;
use crate::{PaxosDispatcher, PaxosEvent, PreVoteRequest, VoteRequest};
use chrono::{DateTime, Duration, Utc};
use spx_lib::true_time::TrueTime;
use spx_lib::write_ahead_log::WriteAheadLog;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

mod accept_timeout_check_watcher;
mod heartbeat_watcher;
mod lease_watcher;
mod write_flush_watcher;

use accept_timeout_check_watcher::AcceptTimeoutCheckState;
use lease_watcher::LeaseState;
use write_flush_watcher::WriteFlushState;

pub use accept_timeout_check_watcher::AcceptTimeoutCheckWatcher;
pub use heartbeat_watcher::{HeartbeatState, HeartbeatWatcher};
pub use lease_watcher::LeaseWatcher;
pub use write_flush_watcher::WriteFlushWatcher;

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

    // Notified by the leader when buffered client writes should be flushed to followers,
    // either because the batch size threshold was reached or the periodic timer fired.
    write_flush: Arc<WriteFlushState>,

    // Notified on a fixed interval so the leader can check for timed-out in-flight batches
    // without queuing behind other pending PaxosEvents.
    accept_timeout_check: Arc<AcceptTimeoutCheckState>,

    // Notified when the heartbeat countdown expires (no client writes for 8 seconds), waking
    // the leader to broadcast a heartbeat accept request to advance t_safe on followers.
    heartbeat: Arc<HeartbeatState>,

    // The cancellation token for the main state machine loop; shared with background tasks
    // spawned by roles so they stop cleanly when the state machine shuts down.
    cancellation_token: CancellationToken,
}

impl PaxosSharedContext {
    pub fn new(
        member_id: Uuid,
        peer_ids: HashSet<Uuid>,
        dispatcher: Arc<dyn PaxosDispatcher>,
        event_tx: Sender<PaxosEvent>,
        cancellation_token: CancellationToken,
    ) -> Self {
        Self {
            member_id,
            peer_ids,
            term: 0,
            t_safe: None,
            lease: LeaseState::new_expired(),
            last_log_term: 0,
            last_log_slot: 0,
            committed_slot: 0,
            uncommitted_logs: BTreeMap::new(),
            lease_length: Duration::seconds(10),
            dispatcher,
            event_tx,
            wal: WriteAheadLog::new(),
            write_flush: WriteFlushState::new(),
            accept_timeout_check: AcceptTimeoutCheckState::new(),
            heartbeat: HeartbeatState::new(),
            cancellation_token,
        }
    }

    pub fn get_cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.clone()
    }

    pub fn lease_watcher(&self) -> LeaseWatcher {
        LeaseWatcher(Arc::clone(&self.lease))
    }

    pub fn accept_timeout_check_watcher(&self) -> AcceptTimeoutCheckWatcher {
        AcceptTimeoutCheckWatcher(Arc::clone(&self.accept_timeout_check))
    }

    pub fn signal_accept_timeout_check_fn(&self) -> impl Fn() + Send + 'static {
        let state = Arc::clone(&self.accept_timeout_check);
        move || state.0.notify_one()
    }

    pub fn write_flush_watcher(&self) -> WriteFlushWatcher {
        WriteFlushWatcher(Arc::clone(&self.write_flush))
    }

    pub fn signal_write_flush(&self) {
        self.write_flush.0.notify_one();
    }

    // Returns a cheap, Send closure that signals the write flush watcher. Used by the leader's
    // periodic timer task which cannot borrow ctx across an await point.
    pub fn signal_write_flush_fn(&self) -> impl Fn() + Send + 'static {
        let state = Arc::clone(&self.write_flush);
        move || state.0.notify_one()
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

    pub fn get_quorum_size(&self) -> usize {
        (self.peer_ids.len() + 1) / 2 + 1
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

    pub async fn extend_leader_lease_expiry_time(&self) {
        self.update_leader_lease_expiry_time(TrueTime::now().latest + self.lease_length)
            .await;
    }

    pub async fn get_leader_lease_expiry(&self) -> Option<DateTime<Utc>> {
        *self.lease.expiry.read().await
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

    pub fn get_t_safe(&self) -> Option<DateTime<Utc>> {
        self.t_safe
    }

    // Advances t_safe to `ts` only if `ts` is later than the current value.
    pub fn advance_t_safe(&mut self, ts: DateTime<Utc>) {
        match self.t_safe {
            None => self.t_safe = Some(ts),
            Some(current) if ts > current => self.t_safe = Some(ts),
            _ => {}
        }
    }

    pub fn heartbeat_watcher(&self) -> HeartbeatWatcher {
        HeartbeatWatcher(Arc::clone(&self.heartbeat))
    }

    pub fn signal_heartbeat(&self) {
        self.heartbeat.0.notify_one();
    }

    pub fn get_heartbeat_state(&self) -> Arc<HeartbeatState> {
        Arc::clone(&self.heartbeat)
    }

    pub fn get_wal(&self) -> &WriteAheadLog {
        &self.wal
    }

    pub fn get_wal_mut(&mut self) -> &mut WriteAheadLog {
        &mut self.wal
    }
}
