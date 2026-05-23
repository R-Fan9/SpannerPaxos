use std::sync::atomic::{AtomicU32, Ordering};

// Tracks a Paxos group member's local WAL log positions
pub struct LogPosition {
    // The index of the last log entry that has been persisted to the member's local WAL
    pub match_index: AtomicU32,

    // The index of the next log entry to be persisted to the member's local WAL
    pub next_index: AtomicU32,
}

impl LogPosition {
    pub fn new() -> Self {
        Self {
            match_index: AtomicU32::new(0),
            next_index: AtomicU32::new(0),
        }
    }

    pub fn from_match_slot(slot: u32) -> Self {
        Self {
            match_index: AtomicU32::new(slot),
            next_index: AtomicU32::new(slot + 1),
        }
    }

    pub fn get_match_index(&self) -> u32 {
        self.match_index.load(Ordering::SeqCst)
    }

    pub fn get_next_index(&self) -> u32 {
        self.next_index.load(Ordering::SeqCst)
    }
}
