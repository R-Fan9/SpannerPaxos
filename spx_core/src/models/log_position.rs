// Tracks a Paxos group member's local WAL log positions
pub struct LogPosition {
    // The slot of the last log entry that has been persisted to the member's local WAL
    pub match_slot: u32,

    // The slot of the next log entry to be persisted to the member's local WAL
    pub next_slot: u32,
}

impl LogPosition {
    pub fn new() -> Self {
        Self {
            match_slot: 0,
            next_slot: 0,
        }
    }

    pub fn with_next_slot(next_slot: u32) -> Self {
        Self {
            match_slot: 0,
            next_slot,
        }
    }
}
