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

impl PreVoteResponse {
    // Returns the reported leader ID if its term satisfies the given condition
    pub fn try_get_leader(&self, term_condition: impl Fn(u32) -> bool) -> Option<Uuid> {
        let leader_id = self.current_leader_id?;
        if term_condition(self.term) {
            Some(leader_id)
        } else {
            None
        }
    }

    pub fn rejection_reason(&self) -> String {
        let mut reasons = Vec::new();
        reasons.push(format!("term {}", self.term));
        if let Some(leader_id) = self.current_leader_id {
            reasons.push(format!("active leader {}", leader_id));
        }
        if self.member_last_log_term.is_some() || self.member_last_log_slot.is_some() {
            let log_term = self.member_last_log_term.map_or("?".to_string(), |t| t.to_string());
            let log_slot = self.member_last_log_slot.map_or("?".to_string(), |s| s.to_string());
            reasons.push(format!("log ahead (term={}, slot={})", log_term, log_slot));
        }
        reasons.join(", ")
    }
}
