use chrono::{DateTime, Utc};

// A payload containing the data for a replicate write operation
#[derive(Clone)]
pub struct ReplicateWritePayload {
    pub term_number: u32,
    pub slot_number: u32,
    pub entry: String,
    pub write_time: DateTime<Utc>,
    pub lease_expiry_time: DateTime<Utc>,
}

impl ReplicateWritePayload {
    pub fn new(
        term_number: u32,
        slot_number: u32,
        entry: String,
        write_time: DateTime<Utc>,
        lease_expiry_time: DateTime<Utc>,
    ) -> Self {
        Self {
            term_number,
            slot_number,
            entry,
            write_time,
            lease_expiry_time,
        }
    }
}
