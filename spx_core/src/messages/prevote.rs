use uuid::Uuid;

#[derive(Clone)]
pub struct PreVoteRequest {
    pub member_id: Uuid,
    pub next_term: u32,
    pub last_log_term: u32,
    pub last_log_slot: u32,
}

#[derive(Clone)]
pub struct PreVoteResponse {
    pub member_id: Uuid,
    pub term: u32,
    pub vote_granted: bool,
    pub current_leader_id: Option<Uuid>,
    pub member_last_log_term: Option<u32>,
    pub member_last_log_slot: Option<u32>,
}
