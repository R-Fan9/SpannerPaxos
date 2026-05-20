use crate::{PaxosEvent, PaxosSharedContext};
use crate::roles::PaxosRole;
use crate::state_machine::PaxosState;
use dashmap::DashMap;
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tonic::async_trait;
use uuid::Uuid;

// A struct that tracks a Paxos group member's local WAL log positions
struct LogPosition {
    // The index of the last log entry that has been persisted to the member's local WAL
    match_index: AtomicU32,

    // The index of the next log entry to be persisted to the member's local WAL
    next_index: AtomicU32,
}

impl LogPosition {
    pub fn new() -> Self {
        Self {
            match_index: AtomicU32::new(0),
            next_index: AtomicU32::new(0),
        }
    }
}

// The state of Paxos group leader
pub struct Leader {
    // The index of the last log entry that has been committed (atomic)
    commit_index: AtomicU32,

    // A concurrent map of Paxos group member IDs to their local WAL positions
    score_board: DashMap<Uuid, LogPosition>,
}

impl Leader {
    pub fn new() -> Self {
        Self {
            commit_index: AtomicU32::new(0),
            score_board: DashMap::new(),
        }
    }

    pub fn get_commit_index(&self) -> u32 {
        self.commit_index.load(Ordering::SeqCst)
    }

    pub fn get_next_index(&self, member_id: Uuid) -> u32 {
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

    pub fn inc_next_index(&self, member_id: Uuid) {
        self.score_board
            .entry(member_id)
            .or_insert_with(LogPosition::new)
            .next_index
            .fetch_add(1, Ordering::SeqCst);
    }

    pub fn update_match_index(&self, index: u32, member_id: Uuid) {
        self.score_board
            .entry(member_id)
            .or_insert_with(LogPosition::new)
            .match_index
            .store(index, Ordering::SeqCst);
    }
}

#[async_trait]
impl PaxosRole for Leader {
    async fn handle_event(
        self,
        event: PaxosEvent,
        ctx: Arc<PaxosSharedContext>,
    ) -> Result<PaxosState, Box<dyn Error + Send + Sync>> {
        todo!()
    }
}
