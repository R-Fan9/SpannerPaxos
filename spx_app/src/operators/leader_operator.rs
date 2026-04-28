use crate::managers::FollowerManager;
use crate::payloads::ReplicateWritePayload;
use chrono::{DateTime, Utc};
use spx_core::states::LeaderState;
use spx_lib::true_time::TrueTimeService;
use spx_lib::write_ahead_log::WriteAheadLogService;
use std::error::Error;
use std::sync::Arc;

// An operator for handling leader-related operations
pub struct LeaderOperator {
    leader: Arc<LeaderState>,
    follower_manager: FollowerManager,
    tt_service: TrueTimeService,
    wal_service: WriteAheadLogService,
}

impl LeaderOperator {
    pub fn new(
        leader: Arc<LeaderState>,
        follower_manager: FollowerManager,
        tt_service: TrueTimeService,
        wal_service: WriteAheadLogService,
    ) -> Self {
        Self {
            leader,
            follower_manager,
            tt_service,
            wal_service,
        }
    }

    pub async fn save_write(&self, entry: String) -> Result<(), Box<dyn Error>> {
        let write_time = self.get_latest_timestamp();
        let slot_number = self.leader.get_next_index(None);

        // Increment the leader's next index (slot number) for the next log entry
        self.leader.inc_next_index(None);

        // Write the current log entry to the leader's local WAL and update its match index
        // to indicate that the log entry has been persisted to the leader's local WAL
        self.wal_service.append(entry.clone()).await;
        self.leader.update_match_index(slot_number, None);

        // Start the Paxos write to replicate the log entry to all followers
        let payload = self.create_replicate_write_payload(entry, slot_number, write_time);
        self.follower_manager
            .replicate_write(payload, |follower_id| {
                self.leader.inc_next_index(Some(follower_id));
            })
            .await?;

        // Start commit wait to ensure the timestamp assigned to the log entry has passed,
        // this is needed to ensure external consistency (linearizability)
        self.tt_service.commit_wait(write_time);

        // Update the leader's commit index to indicate that the log entry has been committed
        self.leader.update_commit_index(slot_number);

        // TODO: Notify the followers to commit the log entry

        Ok(())
    }

    fn get_latest_timestamp(&self) -> DateTime<Utc> {
        self.tt_service.now().latest
    }

    fn create_replicate_write_payload(
        &self,
        entry: String,
        slot_number: u32,
        write_time: DateTime<Utc>,
    ) -> ReplicateWritePayload {
        let term_number = self.leader.get_term_number();
        let lease_expiry_time = self.leader.get_lease_expiry_time();
        ReplicateWritePayload::new(
            term_number,
            slot_number,
            entry,
            write_time,
            lease_expiry_time,
        )
    }
}
