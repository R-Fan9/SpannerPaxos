use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Clone)]
pub struct ReplicateWriteRequest {
    pub term: u32,
    pub slot: u32,
    pub entry: String,
    pub write_time: DateTime<Utc>,
}

#[derive(Clone)]
pub struct ReplicateWriteResponse {
    pub member_id: Uuid,
    pub term: u32,
    pub slot: u32,
}
