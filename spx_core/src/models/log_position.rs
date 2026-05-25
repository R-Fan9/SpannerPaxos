use std::sync::atomic::{AtomicU32, Ordering};

// Tracks a Paxos group member's local WAL log positions
pub struct LogPosition {
    // The slot of the last log entry that has been persisted to the member's local WAL
    pub match_slot: AtomicU32,

    // The slot of the next log entry to be persisted to the member's local WAL
    pub next_slot: AtomicU32,
}

impl LogPosition {
    pub fn new() -> Self {
        Self {
            match_slot: AtomicU32::new(0),
            next_slot: AtomicU32::new(0),
        }
    }

    pub fn from_match_slot(slot: u32) -> Self {
        Self {
            match_slot: AtomicU32::new(slot),
            next_slot: AtomicU32::new(slot + 1),
        }
    }

    pub fn get_match_slot(&self) -> u32 {
        self.match_slot.load(Ordering::SeqCst)
    }

    pub fn get_next_slot(&self) -> u32 {
        self.next_slot.load(Ordering::SeqCst)
    }
}
