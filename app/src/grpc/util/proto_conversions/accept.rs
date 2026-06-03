use chrono::{DateTime, Utc};
use spx_core::{AcceptLogEntry, AcceptRequest, AcceptResponse, ConflictHint};
use uuid::Uuid;

fn millis_to_datetime(ms: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(ms).expect("valid timestamp from proto")
}

pub fn accept_log_entry_to_proto(e: AcceptLogEntry) -> spx_protocol::AcceptLogEntry {
    spx_protocol::AcceptLogEntry {
        term: e.term,
        slot: e.slot,
        entry: e.entry,
    }
}

pub fn accept_log_entry_from_proto(e: spx_protocol::AcceptLogEntry) -> AcceptLogEntry {
    AcceptLogEntry {
        term: e.term,
        slot: e.slot,
        entry: e.entry,
    }
}

pub fn accept_request_to_proto(r: AcceptRequest) -> spx_protocol::AcceptRequest {
    spx_protocol::AcceptRequest {
        term: r.term,
        leader_id: r.leader_id.to_string(),
        prev_log_slot: r.prev_log_slot,
        prev_log_term: r.prev_log_term,
        entries: r.entries.into_iter().map(accept_log_entry_to_proto).collect(),
        leader_commit_slot: r.leader_commit_slot,
        t_send: r.t_send.timestamp_millis(),
    }
}

pub fn accept_request_from_proto(r: spx_protocol::AcceptRequest) -> AcceptRequest {
    AcceptRequest {
        term: r.term,
        leader_id: Uuid::parse_str(&r.leader_id).unwrap(),
        prev_log_slot: r.prev_log_slot,
        prev_log_term: r.prev_log_term,
        entries: r.entries.into_iter().map(accept_log_entry_from_proto).collect(),
        leader_commit_slot: r.leader_commit_slot,
        t_send: millis_to_datetime(r.t_send),
    }
}

pub fn accept_response_to_proto(r: AcceptResponse) -> spx_protocol::AcceptResponse {
    spx_protocol::AcceptResponse {
        member_id: r.member_id.to_string(),
        term: r.term,
        success: r.success,
        last_written_slot: r.last_written_slot,
        conflict_hint: r.conflict_hint.map(|h| spx_protocol::ConflictHint {
            conflict_term: h.conflict_term,
            conflict_first_slot: h.conflict_first_slot,
        }),
        echoed_t_send: r.echoed_t_send.timestamp_millis(),
    }
}

pub fn accept_response_from_proto(r: spx_protocol::AcceptResponse) -> AcceptResponse {
    AcceptResponse {
        member_id: Uuid::parse_str(&r.member_id).unwrap(),
        term: r.term,
        success: r.success,
        last_written_slot: r.last_written_slot,
        conflict_hint: r.conflict_hint.map(|h| ConflictHint {
            conflict_term: h.conflict_term,
            conflict_first_slot: h.conflict_first_slot,
        }),
        echoed_t_send: millis_to_datetime(r.echoed_t_send),
    }
}
