use uuid::Uuid;

#[derive(Clone)]
pub struct VoteRequest {
    pub member_id: Uuid,
    pub next_term: u32,
    pub last_log_term: u32,
    pub last_log_slot: u32,
}

#[derive(Clone)]
pub struct VoteResponse {
    // The ID of the member who responded to the vote request
    pub member_id: Uuid,

    // The receiver's current term is always required, regardless of the outcome
    pub term: u32,

    // The outcome of the vote request
    pub outcome: VoteOutcome,
}

#[derive(Clone)]
pub enum VoteOutcome {
    Promise(VotePromise),
    Rejection(VoteRejection),
}

#[derive(Clone)]
pub struct VotePromise {
    // The slot number of the last log entry the member persisted, needed for score board construction
    pub last_log_slot: u32,

    // Any log entries the follower has not committed, the newly elected leader must process these entries first
    pub uncommitted_entries: Vec<LogEntry>,
}

#[derive(Clone)]
pub struct VoteRejection {
    // Tells the Candidate to abort the election because this leader's TrueTime lease is still active
    pub current_leader_id: Option<Uuid>,

    // Provides a catch-up hint telling the Candidate that it could be saving logs with the wrong term
    pub member_last_log_term: Option<u32>,

    // Provides a catch-up hint telling the Candidate how far behind its logs are compared to this Follower
    pub member_last_log_slot: Option<u32>,

    // Informs the Candidate that it lost the race because this Follower already voted for someone else this term
    pub voted_for_id: Option<Uuid>,
}

#[derive(Clone)]
pub struct LogEntry {
    // The term number when the leader originally proposed this write
    pub term: u32,

    // The slot number of this entry in the append-only log
    pub slot: u32,

    // The actual database command/mutation
    pub entry: String,
}
