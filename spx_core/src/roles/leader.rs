use crate::models::LogPosition;
use crate::roles::PaxosRole;
use crate::state_machine::PaxosState;
use crate::{AcceptLogEntry, AcceptRequest, PaxosEvent, PaxosSharedContext};
use spx_lib::write_ahead_log::WriteAheadLog;
use std::collections::HashMap;
use std::error::Error;
use tonic::async_trait;
use uuid::Uuid;

// The state of Paxos group leader
pub struct Leader {
    // A map of Paxos group member IDs to their local WAL positions
    score_board: HashMap<Uuid, LogPosition>,
}

impl Leader {
    pub fn new(score_board: HashMap<Uuid, LogPosition>) -> Self {
        Self { score_board }
    }

    pub async fn process_uncommitted_logs(
        &mut self,
        ctx: &mut PaxosSharedContext,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let current_term = ctx.get_current_term();

        // BTreeMap iterates in slot order, so no explicit sort is needed.
        let entries: Vec<_> = ctx.get_uncommitted_logs().values().cloned().collect();

        let wal = ctx.get_wal_mut();
        for entry in &entries {
            wal.append(entry.slot, current_term, entry.entry.clone());
        }

        // Advance ctx's log position to the highest slot written
        if let Some(last) = entries.last() {
            ctx.set_last_log_slot(last.slot);
            ctx.set_last_log_term(current_term);

            // Update every peer's next_slot to just past the leader's last log slot
            let next_slot = last.slot + 1;
            for position in self.score_board.values_mut() {
                position.next_slot = next_slot;
            }
        }

        self.dispatch_accept_request(ctx).await
    }

    pub async fn dispatch_accept_request(
        &self,
        ctx: &PaxosSharedContext,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let wal = ctx.get_wal();
        let current_term = ctx.get_current_term();
        let committed_slot = ctx.get_committed_slot();

        for (&member_id, position) in &self.score_board {
            let next_slot = position.next_slot;

            // prev_log_slot is the entry just before what we're about to send;
            // slot 0 is a sentinel meaning "no predecessor", in which case term is also 0
            let prev_log_slot = next_slot.saturating_sub(1);
            let prev_log_term = if prev_log_slot == 0 { 0 } else { wal.get_term(prev_log_slot) };

            // Collect all WAL entries from next_slot onwards; empty when the follower is caught up
            let entries = wal
                .get_entries_from(next_slot)
                .into_iter()
                .map(|(slot, term, command)| AcceptLogEntry {
                    term: term as u64,
                    slot: slot as u64,
                    command: command.into_bytes(),
                })
                .collect();

            let request = AcceptRequest {
                term: current_term as u64,
                leader_id: ctx.get_current_member_id(),
                prev_log_slot: prev_log_slot as u64,
                prev_log_term: prev_log_term as u64,
                entries,
                leader_commit_slot: committed_slot as u64,
            };

            ctx.get_dispatcher()
                .dispatch_accept_request(member_id, request)
                .await?;
        }
        Ok(())
    }

    fn get_next_slot(&self, member_id: Uuid) -> u32 {
        self.score_board.get(&member_id).map_or(0, |p| p.next_slot)
    }

    fn has_quorum(&self, slot_number: u32) -> bool {
        let num_matched = self
            .score_board
            .values()
            .filter(|pos| pos.match_slot >= slot_number)
            .count();

        // + 1 accounts for the leader's implicit self-match, which is not in the score board
        (num_matched + 1) >= (self.score_board.len() + 1) / 2 + 1
    }

    fn inc_next_slot(&mut self, member_id: Uuid) {
        if let Some(pos) = self.score_board.get_mut(&member_id) {
            pos.next_slot += 1;
        }
    }

    fn update_match_slot(&mut self, slot: u32, member_id: Uuid) {
        if let Some(pos) = self.score_board.get_mut(&member_id) {
            pos.match_slot = slot;
        }
    }
}

#[async_trait]
impl PaxosRole for Leader {
    async fn handle_event(
        self,
        _event: PaxosEvent,
        _ctx: &mut PaxosSharedContext,
    ) -> Result<PaxosState, Box<dyn Error + Send + Sync>> {
        todo!()
    }
}
