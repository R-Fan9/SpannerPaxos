use uuid::Uuid;

#[derive(Clone)]
pub struct AcceptRequest {
    // The leader's current term
    pub term: u64,

    // The leader's unique identifier
    pub leader_id: Uuid,

    // The slot of the log entry immediately preceding the entries array
    pub prev_log_slot: u64,

    // The term of the preceding log entry — follower rejects if this mismatches its own disk
    pub prev_log_term: u64,

    // The log entries being replicated (empty for heartbeat/lease renewal)
    pub entries: Vec<AcceptLogEntry>,

    // The highest slot the leader knows has reached a global quorum
    pub leader_commit_slot: u64,
}

#[derive(Clone)]
pub struct AcceptResponse {
    // The follower's current term — leader steps down if this exceeds its own term
    pub term: u64,

    // True if the anchor matched and entries were written; false if rejected
    pub success: bool,

    // The highest slot the follower successfully wrote to disk (populated when success == true)
    pub last_written_slot: u64,

    // Diagnostic hint for fast backtrack (populated when success == false)
    pub conflict_hint: Option<ConflictHint>,
}

#[derive(Clone)]
pub struct ConflictHint {
    // The term of the conflicting entry sitting at the requested anchor slot
    pub conflict_term: u64,

    // The first slot on the follower's disk where this conflicting term began
    pub conflict_first_slot: u64,
}

#[derive(Clone)]
pub struct AcceptLogEntry {
    // The term when this log was originally created
    pub term: u64,

    // The global slot position of this log in the history
    pub slot: u64,

    // The actual database command (serialized bytes)
    pub command: Vec<u8>,
}
