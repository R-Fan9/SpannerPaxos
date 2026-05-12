use uuid::Uuid;

#[derive(Clone)]
pub struct VoteRequest {
    pub member_id: Uuid,
    pub next_term: u32,
    pub last_log_term: u32,
    pub last_log_slot: u32,
}
