use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Clone)]
pub struct AcceptRequest {
    // The leader's current term
    pub term: u32,

    // The leader's unique identifier
    pub leader_id: Uuid,

    // The slot of the log entry immediately preceding the entries array
    pub prev_log_slot: u32,

    // The term of the preceding log entry — follower rejects if this mismatches its own disk
    pub prev_log_term: u32,

    // The log entries being replicated (empty for heartbeat/lease renewal)
    pub entries: Vec<AcceptLogEntry>,

    // The highest slot the leader knows has reached a global quorum
    pub leader_commit_slot: u32,

    // The TrueTime earliest bound at which the leader sent this request
    pub t_send: DateTime<Utc>,

    // A lower-bound promise on the timestamp that will be assigned to slot n+1, where n is the
    // highest slot in this request. The next write will carry a timestamp >= this value.
    // None when the leader is sending entries with no explicit timestamp promise (followers use
    // the committed entry's timestamp to advance t_safe instead).
    pub min_next_ts: Option<DateTime<Utc>>,
}

#[derive(Clone)]
pub struct AcceptResponse {
    // The unique identifier of the member sending this response
    pub member_id: Uuid,

    // The follower's current term — leader steps down if this exceeds its own term
    pub term: u32,

    // True if the anchor matched and entries were written; false if rejected
    pub success: bool,

    // The highest slot the follower successfully wrote to disk (populated when success == true)
    pub last_written_slot: u32,

    // Diagnostic hint for fast backtrack (populated when success == false)
    pub conflict_hint: Option<ConflictHint>,

    // The t_send value from the originating AcceptRequest, echoed back for leader lease tracking
    pub echoed_t_send: DateTime<Utc>,
}

#[derive(Clone)]
pub struct ConflictHint {
    // The term of the conflicting entry sitting at the requested anchor slot
    pub conflict_term: u32,

    // The first slot on the follower's disk where this conflicting term began
    pub conflict_first_slot: u32,
}

#[derive(Clone)]
pub struct AcceptLogEntry {
    // The term when this log was originally created
    pub term: u32,

    // The global slot position of this log in the history
    pub slot: u32,

    // The value stored at this log slot
    pub entry: String,

    // The TrueTime latest bound at the moment the leader appended this entry
    pub timestamp: DateTime<Utc>,
}
