use chrono::{DateTime, Utc};
use std::collections::HashMap;
use uuid::Uuid;

// A struct that tracks a Paxos group member's local WAL log positions
struct WalPosition {
    // The index of the last log entry that has been persisted to the member's local WAL
    match_index: Option<u32>,

    // The index of the next log entry to be persisted to the member's local WAL
    next_index: Option<u32>,
}

// Paxos Group Leader
pub struct Leader {
    // The unique identifier for the leader
    id: Uuid,

    // The term (ballot) number of the leader
    term_number: u32,

    // The index of the last log entry that has been committed
    commit_index: Option<u32>,

    // Time at which the leader is safe to serve a read request
    t_safe: DateTime<Utc>,

    // The time at which the leader's lease expires
    lease_expiry_time: DateTime<Utc>,

    // A map of Paxos group member IDs to their local WAL positions
    score_board: HashMap<Uuid, WalPosition>,
}

impl Leader {
    pub fn get_term_number(&self) -> u32 {
        self.term_number
    }

    pub fn get_commit_index(&self) -> u32 {
        self.commit_index.unwrap_or(0)
    }

    pub fn get_lease_expiry_time(&self) -> DateTime<Utc> {
        self.lease_expiry_time
    }

    pub fn get_next_index(&self, member_id: Option<Uuid>) -> u32 {
        let member_id = member_id.unwrap_or(self.id);
        let log_position = self.score_board.get(&member_id).unwrap();
        log_position.next_index.unwrap_or(0)
    }

    pub fn has_quorum(&self, slot_number: u32) -> bool {
        let num_matched = self
            .score_board
            .values()
            .filter(|log| log.match_index.unwrap_or(0) >= slot_number)
            .count();

        num_matched >= (self.score_board.len() / 2)
    }

    pub fn has_committed(&self, slot_number: u32) -> bool {
        slot_number <= self.get_commit_index()
    }

    pub fn update_commit_index(&mut self, index: u32) {
        self.commit_index = Some(index);
    }

    pub fn inc_next_index(&mut self, member_id: Option<Uuid>) {
        let member_id = member_id.unwrap_or(self.id);
        let log_position = self.score_board.get_mut(&member_id).unwrap();
        log_position.next_index = Some(log_position.next_index.unwrap_or(0) + 1);
    }

    pub fn update_match_index(&mut self, index: u32, member_id: Option<Uuid>) {
        let member_id = member_id.unwrap_or(self.id);
        let log_position = self.score_board.get_mut(&member_id).unwrap();
        log_position.match_index = Some(index);
    }
}
