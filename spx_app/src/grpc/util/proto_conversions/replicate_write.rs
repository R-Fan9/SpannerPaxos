use chrono::{DateTime, TimeZone, Utc};
use spx_core::{ReplicateWriteRequest, ReplicateWriteResponse};
use spx_protocol::{ReplicateWriteRequest as ProtoRequest, ReplicateWriteResponse as ProtoResponse};
use uuid::Uuid;

pub fn replicate_write_request_to_proto(req: ReplicateWriteRequest) -> tonic::Request<ProtoRequest> {
    tonic::Request::new(ProtoRequest {
        term: req.term,
        slot: req.slot,
        entry: req.entry,
        write_time: Some(prost_types::Timestamp {
            seconds: req.write_time.timestamp(),
            nanos: req.write_time.timestamp_subsec_nanos() as i32,
        }),
    })
}

pub fn replicate_write_request_from_proto(req: ProtoRequest) -> ReplicateWriteRequest {
    ReplicateWriteRequest {
        term: req.term,
        slot: req.slot,
        entry: req.entry,
        write_time: timestamp_to_datetime(req.write_time),
    }
}

pub fn replicate_write_response_to_proto(resp: ReplicateWriteResponse) -> tonic::Request<ProtoResponse> {
    tonic::Request::new(ProtoResponse {
        member_id: resp.member_id.to_string(),
        term: resp.term,
        slot: resp.slot,
    })
}

pub fn replicate_write_response_from_proto(resp: ProtoResponse) -> ReplicateWriteResponse {
    ReplicateWriteResponse {
        member_id: Uuid::parse_str(&resp.member_id).expect("invalid member_id UUID"),
        term: resp.term,
        slot: resp.slot,
    }
}

fn timestamp_to_datetime(ts: Option<prost_types::Timestamp>) -> DateTime<Utc> {
    ts.map(|t| Utc.timestamp_opt(t.seconds, t.nanos as u32).single())
        .flatten()
        .unwrap_or_else(Utc::now)
}
