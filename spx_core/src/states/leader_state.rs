use chrono::{DateTime, Utc};
use dashmap::DashMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU32, Ordering};
use uuid::Uuid;

// A struct that tracks a Paxos group member's local WAL log positions
pub struct LogPosition {
    // The index of the last log entry that has been persisted to the member's local WAL
    pub match_index: AtomicU32,

    // The index of the next log entry to be persisted to the member's local WAL
    pub next_index: AtomicU32,
}

impl LogPosition {
    fn new() -> Self {
        Self {
            match_index: AtomicU32::new(0),
            next_index: AtomicU32::new(0),
        }
    }
}

// Paxos Group Leader - Thread-safe without external locking
pub struct LeaderState {
    // The unique identifier for the leader (immutable, no synchronization needed)
    id: Uuid,

    // The term (ballot) number of the leader (atomic for lock-free access)
    term_number: AtomicU32,

    // The index of the last log entry that has been committed (atomic)
    commit_index: AtomicU32,

    // Time at which the leader is safe to serve a read request (RwLock for rare writes)
    t_safe: RwLock<DateTime<Utc>>,

    // The time at which the leader's lease expires (RwLock for rare writes)
    lease_expiry_time: RwLock<DateTime<Utc>>,

    // A concurrent map of Paxos group member IDs to their local WAL positions
    // DashMap provides lock-free concurrent access
    score_board: DashMap<Uuid, LogPosition>,
}

impl LeaderState {
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            term_number: AtomicU32::new(0),
            commit_index: AtomicU32::new(0),
            t_safe: RwLock::new(now),
            lease_expiry_time: RwLock::new(now),
            score_board: DashMap::new(),
        }
    }

    pub fn get_term_number(&self) -> u32 {
        self.term_number.load(Ordering::SeqCst)
    }

    pub fn get_commit_index(&self) -> u32 {
        self.commit_index.load(Ordering::SeqCst)
    }

    pub fn get_lease_expiry_time(&self) -> DateTime<Utc> {
        *self.lease_expiry_time.read().unwrap()
    }

    pub fn get_next_index(&self, member_id: Option<Uuid>) -> u32 {
        let member_id = member_id.unwrap_or(self.id);
        self.score_board
            .entry(member_id)
            .or_insert_with(LogPosition::new)
            .next_index
            .load(Ordering::SeqCst)
    }

    pub fn has_quorum(&self, slot_number: u32) -> bool {
        let num_matched = self
            .score_board
            .iter()
            .filter(|entry| entry.value().match_index.load(Ordering::SeqCst) >= slot_number)
            .count();

        num_matched >= (self.score_board.len() / 2 + 1)
    }

    pub fn has_committed(&self, slot_number: u32) -> bool {
        slot_number <= self.get_commit_index()
    }

    pub fn update_commit_index(&self, index: u32) {
        self.commit_index.store(index, Ordering::SeqCst);
    }

    pub fn inc_next_index(&self, member_id: Option<Uuid>) {
        let member_id = member_id.unwrap_or(self.id);
        self.score_board
            .entry(member_id)
            .or_insert_with(LogPosition::new)
            .next_index
            .fetch_add(1, Ordering::SeqCst);
    }

    pub fn update_match_index(&self, index: u32, member_id: Option<Uuid>) {
        let member_id = member_id.unwrap_or(self.id);
        self.score_board
            .entry(member_id)
            .or_insert_with(LogPosition::new)
            .match_index
            .store(index, Ordering::SeqCst);
    }
}

impl Default for LeaderState {
    fn default() -> Self {
        Self::new()
    }
}
