use crate::{PreVoteRequest, PreVoteResponse, PaxosSharedContext};
use std::sync::Arc;
use uuid::Uuid;

// Returns the active leader ID from the response if its term satisfies the given condition
pub(super) fn get_active_leader(
    response: &PreVoteResponse,
    term_condition: impl Fn(u32) -> bool,
) -> Option<Uuid> {
    let leader_id = response.current_leader_id?;
    if term_condition(response.term) {
        Some(leader_id)
    } else {
        None
    }
}

pub(super) fn handle_pre_vote_request(
    request: PreVoteRequest,
    ctx: Arc<PaxosSharedContext>,
) -> PreVoteResponse {
    let current_term = ctx.get_current_term();
    let current_member_id = ctx.get_current_member_id();

    // Reject if the next term number proposal is less than or equal to the current term number
    if request.next_term <= current_term {
        return PreVoteResponse {
            term: current_term,
            member_id: current_member_id,
            vote_granted: false,
            current_leader_id: None,
            member_last_log_term: None,
            member_last_log_slot: None,
        };
    }

    // Reject if the term number of the last log entry persisted by the candidate is lower than the term number of the last log persisted by the current member (node)
    let current_last_log_term = ctx.get_last_log_term();
    if request.last_log_term < current_term {
        return PreVoteResponse {
            term: current_term,
            member_id: current_member_id,
            vote_granted: false,
            current_leader_id: None,
            member_last_log_term: Some(current_last_log_term),
            member_last_log_slot: None,
        };
    }

    // Reject if the slot number of the last log entry persisted by the candidate is lower than the slot number of the last log persisted by the current member (node)
    let current_last_log_slot = ctx.get_last_log_slot();
    if request.last_log_slot < current_last_log_slot {
        return PreVoteResponse {
            term: current_term,
            member_id: current_member_id,
            vote_granted: false,
            current_leader_id: None,
            member_last_log_term: Some(current_last_log_term),
            member_last_log_slot: Some(current_last_log_slot),
        };
    }

    PreVoteResponse {
        term: current_term,
        member_id: current_member_id,
        vote_granted: true,
        current_leader_id: None,
        member_last_log_term: None,
        member_last_log_slot: None,
    }
}
