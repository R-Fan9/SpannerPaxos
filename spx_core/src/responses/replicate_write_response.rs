use uuid::Uuid;

pub struct ReplicateWriteResponse {
    pub term_number: u32,
    pub slot_number: u32,
    pub follower_id: Uuid,
}
