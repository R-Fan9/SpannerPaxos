use spx_core::{PreVoteRequest, PreVoteResponse, VoteRequest};
use uuid::Uuid;

pub fn pre_vote_request_to_proto(r: PreVoteRequest) -> spx_protocol::PreVoteRequest {
    spx_protocol::PreVoteRequest {
        member_id: r.member_id.to_string(),
        next_term: r.next_term,
        last_log_term: r.last_log_term,
        last_log_slot: r.last_log_slot,
    }
}

pub fn pre_vote_request_from_proto(r: spx_protocol::PreVoteRequest) -> PreVoteRequest {
    PreVoteRequest {
        member_id: Uuid::parse_str(&r.member_id).unwrap(),
        next_term: r.next_term,
        last_log_term: r.last_log_term,
        last_log_slot: r.last_log_slot,
    }
}

pub fn pre_vote_response_to_proto(r: PreVoteResponse) -> spx_protocol::PreVoteResponse {
    spx_protocol::PreVoteResponse {
        member_id: r.member_id.to_string(),
        term: r.term,
        vote_granted: r.vote_granted,
        current_leader_id: r.current_leader_id.map(|id| id.to_string()),
        member_last_log_term: r.member_last_log_term,
        member_last_log_slot: r.member_last_log_slot,
    }
}

pub fn pre_vote_response_from_proto(r: spx_protocol::PreVoteResponse) -> PreVoteResponse {
    PreVoteResponse {
        member_id: Uuid::parse_str(&r.member_id).unwrap(),
        term: r.term,
        vote_granted: r.vote_granted,
        current_leader_id: r.current_leader_id.map(|id| Uuid::parse_str(&id).unwrap()),
        member_last_log_term: r.member_last_log_term,
        member_last_log_slot: r.member_last_log_slot,
    }
}

pub fn vote_request_to_proto(r: VoteRequest) -> spx_protocol::RequestVoteRequest {
    spx_protocol::RequestVoteRequest {
        member_id: r.member_id.to_string(),
        next_term: r.next_term,
        last_log_term: r.last_log_term,
        last_log_slot: r.last_log_slot,
    }
}
