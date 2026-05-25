use crate::models::{LogEntry, LogPosition};
use crate::roles::PaxosRole;
use crate::state_machine::PaxosState;
use crate::{PaxosEvent, PaxosSharedContext, ReplicateWriteRequest};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tonic::async_trait;
use uuid::Uuid;

// The state of Paxos group leader
pub struct Leader {
    // A concurrent map of Paxos group member IDs to their local WAL positions
    score_board: Arc<DashMap<Uuid, LogPosition>>,
}

impl Leader {
    pub fn new(score_board: DashMap<Uuid, LogPosition>) -> Self {
        Self {
            score_board: Arc::new(score_board),
        }
    }

    pub fn process_uncommitted_logs(&self, entries: Vec<LogEntry>) {
        todo!()
    }

    pub async fn dispatch_replicate_write(
        &self,
        ctx: Arc<PaxosSharedContext>,
        slot: u32,
        entry: String,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        todo!()
        // let request = ReplicateWriteRequest {
        //     term: ctx.get_current_term(),
        //     slot,
        //     entry,
        //     write_time,
        //     leader_lease_expiry_time,
        // };
        //
        // let score_board = self.score_board.clone();
        // ctx.get_dispatcher()
        //     .dispatch_replicate_write_request(
        //         request,
        //         Arc::new(move |member_id| {
        //             score_board
        //                 .entry(member_id)
        //                 .or_insert_with(LogPosition::new)
        //                 .next_slot
        //                 .fetch_add(1, Ordering::SeqCst);
        //         }),
        //     )
        //     .await
    }

    fn get_next_slot(&self, member_id: Uuid) -> u32 {
        self.score_board
            .entry(member_id)
            .or_insert_with(LogPosition::new)
            .get_next_slot()
    }

    fn has_quorum(&self, slot_number: u32) -> bool {
        let num_matched = self
            .score_board
            .iter()
            .filter(|entry| entry.value().get_match_slot() >= slot_number)
            .count();

        num_matched >= (self.score_board.len() / 2 + 1)
    }

    fn inc_next_slot(&self, member_id: Uuid) {
        self.score_board
            .entry(member_id)
            .or_insert_with(LogPosition::new)
            .next_slot
            .fetch_add(1, Ordering::SeqCst);
    }

    fn update_match_slot(&self, slot: u32, member_id: Uuid) {
        self.score_board
            .entry(member_id)
            .or_insert_with(LogPosition::new)
            .match_slot
            .store(slot, Ordering::SeqCst);
    }
}

#[async_trait]
impl PaxosRole for Leader {
    async fn handle_event(
        self,
        _event: PaxosEvent,
        _ctx: Arc<PaxosSharedContext>,
    ) -> Result<PaxosState, Box<dyn Error + Send + Sync>> {
        todo!()
    }
}
