use crate::models::LogEntry;
use uuid::Uuid;

#[derive(Clone)]
pub struct VoteRequest {
    pub member_id: Uuid,
    pub term: u32,
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

impl VoteResponse {
    // Returns the reported leader ID from a rejection if its term satisfies the given condition
    pub fn try_get_leader(&self, term_condition: impl Fn(u32) -> bool) -> Option<Uuid> {
        let VoteOutcome::Rejection(rejection) = &self.outcome else {
            return None;
        };
        let leader_id = rejection.current_leader_id?;
        if term_condition(self.term) {
            Some(leader_id)
        } else {
            None
        }
    }
}

#[derive(Clone)]
pub enum VoteOutcome {
    Promise(VotePromise),
    Rejection(VoteRejection),
}

#[derive(Clone)]
pub struct VotePromise {
    // The term number of the last log entry the member persisted, needed for score board construction
    pub last_log_term: u32,

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

impl VoteRejection {
    pub fn rejection_reason(&self, term: u32) -> String {
        let mut reasons = Vec::new();
        reasons.push(format!("term {}", term));
        if let Some(leader_id) = self.current_leader_id {
            reasons.push(format!("active leader {}", leader_id));
        }
        if self.member_last_log_term.is_some() || self.member_last_log_slot.is_some() {
            let log_term = self
                .member_last_log_term
                .map_or("?".to_string(), |t| t.to_string());
            let log_slot = self
                .member_last_log_slot
                .map_or("?".to_string(), |s| s.to_string());
            reasons.push(format!("log ahead (term={}, slot={})", log_term, log_slot));
        }
        if let Some(voted_for) = self.voted_for_id {
            reasons.push(format!("already voted for {}", voted_for));
        }
        reasons.join(", ")
    }
}
