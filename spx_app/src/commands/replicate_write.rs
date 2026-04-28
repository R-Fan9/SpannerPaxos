use prost_types::Timestamp;
use crate::payloads::ReplicateWritePayload;
use spx_protocol::ReplicateWriteRequest;
use std::error::Error;
use tokio::sync::mpsc;

// A command used to signal the completion of a replicate write operation from a follower
pub struct ReplicateWriteCommand {
    // The payloads of the log entry
    payload: ReplicateWritePayload,

    // A channel to signal a quorum of followers has replicated the log entry
    quorum_tx: mpsc::Sender<Result<(), Box<dyn Error + Send + Sync>>>,
}

impl ReplicateWriteCommand {
    pub fn new(
        payload: ReplicateWritePayload,
        quorum_tx: mpsc::Sender<Result<(), Box<dyn Error + Send + Sync>>>,
    ) -> Self {
        Self { payload, quorum_tx }
    }

    // Sends the result of the replicate write operation
    pub async fn send(
        &self,
        resp: Result<(), Box<dyn Error + Send + Sync>>,
    ) -> Result<(), Box<dyn Error>> {
        self.quorum_tx.send(resp).await?;
        Ok(())
    }

    // Creates a replicate write request to be sent to the follower
    pub fn create_request(&self) -> ReplicateWriteRequest {
        ReplicateWriteRequest {
            term_number: self.payload.term_number,
            slot_number: self.payload.slot_number,
            entry: self.payload.entry.clone(),
            write_time: Some(Timestamp {
                seconds: self.payload.write_time.timestamp(),
                nanos: self.payload.write_time.timestamp_subsec_nanos() as i32,
            }),
            lease_expiry_time: Some(Timestamp {
                seconds: self.payload.lease_expiry_time.timestamp(),
                nanos: self.payload.lease_expiry_time.timestamp_subsec_nanos() as i32,
            }),
        }
    }
}
