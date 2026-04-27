use crate::states::LeaderState;
use std::error::Error;
use std::sync::Arc;
use uuid::Uuid;

// A processor for handling responses from followers to replicate write requests
#[derive(Clone)]
pub struct ReplicateWriteProcessor {
    leader_state: Arc<LeaderState>,
}

impl ReplicateWriteProcessor {
    pub fn quorum_reached(
        &self,
        follower_id: Uuid,
        term: u32,
        slot: u32,
    ) -> Result<bool, Box<dyn Error>> {
        if term < self.leader_state.get_term_number() {
            // The response from the follower is for an older term
            return Err("Follower response is for an older term".into());
        }

        // Update the follower's match index to indicate that the log entry has been persisted by the follower
        self.leader_state
            .update_match_index(slot, Some(follower_id));

        if self.leader_state.has_committed(slot) {
            // The log entry at the given slot number has already been committed, return true
            return Ok(true);
        }

        if self.leader_state.has_quorum(slot) {
            return Ok(true);
        }

        Ok(false)
    }
}
