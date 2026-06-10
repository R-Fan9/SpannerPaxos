use chrono::{DateTime, Utc};
use spx_core::{LogEntry, VoteOutcome, VotePromise, VoteRejection, VoteRequest, VoteResponse};
use uuid::Uuid;

fn log_entry_to_proto(e: LogEntry) -> spx_protocol::LogEntry {
    spx_protocol::LogEntry {
        term: e.term,
        slot: e.slot,
        entry: e.entry,
    }
}

fn log_entry_from_proto(e: spx_protocol::LogEntry) -> LogEntry {
    LogEntry {
        term: e.term,
        slot: e.slot,
        entry: e.entry,
        timestamp: DateTime::<Utc>::UNIX_EPOCH,
    }
}

pub fn vote_request_to_proto(r: VoteRequest) -> spx_protocol::RequestVoteRequest {
    spx_protocol::RequestVoteRequest {
        member_id: r.member_id.to_string(),
        term: r.term,
        last_log_term: r.last_log_term,
        last_log_slot: r.last_log_slot,
    }
}

pub fn vote_request_from_proto(r: spx_protocol::RequestVoteRequest) -> VoteRequest {
    VoteRequest {
        member_id: Uuid::parse_str(&r.member_id).unwrap(),
        term: r.term,
        last_log_term: r.last_log_term,
        last_log_slot: r.last_log_slot,
    }
}

pub fn vote_response_to_proto(r: VoteResponse) -> spx_protocol::RequestVoteResponse {
    let outcome = match r.outcome {
        VoteOutcome::Promise(promise) => {
            spx_protocol::request_vote_response::Outcome::Promise(spx_protocol::Promise {
                last_log_term: promise.last_log_term,
                last_log_slot: promise.last_log_slot,
                uncommitted_entries: promise.uncommitted_entries.into_iter().map(log_entry_to_proto).collect(),
            })
        }
        VoteOutcome::Rejection(rejection) => {
            spx_protocol::request_vote_response::Outcome::Rejection(spx_protocol::Rejection {
                current_leader_id: rejection.current_leader_id.map(|id| id.to_string()),
                member_last_log_term: rejection.member_last_log_term,
                member_last_log_slot: rejection.member_last_log_slot,
                voted_for_id: rejection.voted_for_id.map(|id| id.to_string()),
            })
        }
    };

    spx_protocol::RequestVoteResponse {
        member_id: r.member_id.to_string(),
        term: r.term,
        outcome: Some(outcome),
    }
}

pub fn vote_response_from_proto(r: spx_protocol::RequestVoteResponse) -> VoteResponse {
    let member_id = Uuid::parse_str(&r.member_id).unwrap();
    let term = r.term;

    let outcome = match r.outcome {
        Some(spx_protocol::request_vote_response::Outcome::Promise(promise)) => {
            VoteOutcome::Promise(VotePromise {
                last_log_term: promise.last_log_term,
                last_log_slot: promise.last_log_slot,
                uncommitted_entries: promise
                    .uncommitted_entries
                    .into_iter()
                    .map(log_entry_from_proto)
                    .collect(),
            })
        }
        Some(spx_protocol::request_vote_response::Outcome::Rejection(rejection)) => {
            VoteOutcome::Rejection(VoteRejection {
                current_leader_id: rejection.current_leader_id.map(|id| Uuid::parse_str(&id).unwrap()),
                member_last_log_term: rejection.member_last_log_term,
                member_last_log_slot: rejection.member_last_log_slot,
                voted_for_id: rejection.voted_for_id.map(|id| Uuid::parse_str(&id).unwrap()),
            })
        }
        None => panic!("Unexpected: VoteResponse outcome is None"),
    };

    VoteResponse {
        member_id,
        term,
        outcome,
    }
}
