use crate::models::LogPosition;
use crate::roles::PaxosRole;
use crate::state_machine::PaxosState;
use crate::{AcceptLogEntry, AcceptRequest, AcceptResponse, PaxosEvent, PaxosSharedContext};
use futures::future::try_join_all;
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
        // Restamp each entry to the current term and collect a snapshot for the WAL writes.
        let entries: Vec<_> = ctx
            .get_uncommitted_logs_mut()
            .values_mut()
            .map(|entry| {
                entry.term = current_term;
                entry.clone()
            })
            .collect();

        let wal = ctx.get_wal_mut();
        for entry in &entries {
            wal.append(entry.slot, entry.term, entry.entry.clone());
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

    async fn dispatch_accept_request(
        &self,
        ctx: &PaxosSharedContext,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let futures = self
            .score_board
            .keys()
            .copied()
            .map(|member_id| self.dispatch_accept_request_to_member(member_id, ctx));
        try_join_all(futures).await?;
        Ok(())
    }

    async fn dispatch_accept_request_to_member(
        &self,
        member_id: Uuid,
        ctx: &PaxosSharedContext,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let position = match self.score_board.get(&member_id) {
            Some(pos) => pos,
            None => return Ok(()),
        };

        let wal = ctx.get_wal();
        let next_slot = position.next_slot;

        // prev_log_slot is the entry just before what we're about to send;
        // slot 0 is a sentinel meaning "no predecessor", in which case term is also 0
        let prev_log_slot = next_slot.saturating_sub(1);
        let prev_log_term = if prev_log_slot == 0 {
            0
        } else {
            wal.get_term(prev_log_slot)
                .expect("prev_log_slot derived from next_slot which was set from a written entry")
        };

        // Collect all WAL entries from next_slot onwards; empty when the follower is caught up
        let entries = wal
            .get_entries_from(next_slot)
            .into_iter()
            .map(|(slot, term, command)| AcceptLogEntry {
                term,
                slot,
                entry: command,
            })
            .collect();

        let request = AcceptRequest {
            term: ctx.get_current_term(),
            leader_id: ctx.get_current_member_id(),
            prev_log_slot,
            prev_log_term,
            entries,
            leader_commit_slot: ctx.get_committed_slot(),
        };

        ctx.get_dispatcher()
            .dispatch_accept_request(member_id, request)
            .await
    }

    async fn handle_accept_response(
        &mut self,
        response: AcceptResponse,
        ctx: &mut PaxosSharedContext,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let member_id = response.member_id;
        let current_term = ctx.get_current_term();

        if response.term < current_term {
            println!(
                "{} Info: Ignoring accept response from {} with stale term {} (current: {})",
                ctx.log_prefix("Leader"),
                member_id,
                response.term,
                current_term
            );
            return Ok(());
        }

        if response.success {
            // Follower successfully appended — advance its position in the score board
            let last_written_slot = response.last_written_slot;
            {
                let pos = self
                    .score_board
                    .get_mut(&member_id)
                    .expect("accept response received from unknown member");
                if last_written_slot > pos.match_slot {
                    pos.match_slot = last_written_slot;
                    pos.next_slot = last_written_slot + 1;
                }
            }

            // Check whether a quorum has replicated enough to advance the committed slot.
            // Quorum size is the majority of total members (followers + leader).
            let total_members = self.score_board.len() + 1;
            let quorum_size = total_members / 2 + 1;

            // Gather the top (quorum_size - 1) follower match_slots plus the leader's own
            // last_log_slot, then take the minimum — the highest slot guaranteed on a quorum.
            let mut follower_slots: Vec<u32> =
                self.score_board.values().map(|p| p.match_slot).collect();
            follower_slots.sort_unstable_by(|a, b| b.cmp(a));
            let min_quorum_slot = follower_slots
                .into_iter()
                .take(quorum_size - 1)
                .chain(std::iter::once(ctx.get_last_log_slot()))
                .min()
                .unwrap_or(0);

            if min_quorum_slot > ctx.get_committed_slot() {
                ctx.set_committed_slot(min_quorum_slot);
                ctx.get_uncommitted_logs_mut().retain(|&slot, _| slot > min_quorum_slot);
            }

            return Ok(());
        }

        let Some(hint) = response.conflict_hint else {
            return Ok(());
        };

        let pos = self
            .score_board
            .get_mut(&member_id)
            .expect("accept response received from unknown member");

        if hint.conflict_term == 0 {
            // Follower log is too short — back up to the first missing slot
            pos.next_slot = hint.conflict_first_slot;
        } else {
            // Term mismatch at anchor slot
            pos.next_slot = match ctx.get_wal().find_highest_slot_for_term(hint.conflict_term) {
                // Leader has entries for this term — align just past its last slot
                Some(leader_last_slot) => leader_last_slot + 1,
                // Leader has no entries for this term — skip to follower's hint
                None => hint.conflict_first_slot,
            };
        }

        self.dispatch_accept_request_to_member(member_id, ctx).await
    }

    fn has_quorum(&self, slot: u32) -> bool {
        let num_matched = self
            .score_board
            .values()
            .filter(|pos| pos.match_slot >= slot)
            .count();

        // + 1 accounts for the leader's implicit self-match, which is not in the score board
        (num_matched + 1) >= (self.score_board.len() + 1) / 2 + 1
    }
}

#[async_trait]
impl PaxosRole for Leader {
    async fn handle_event(
        mut self,
        event: PaxosEvent,
        ctx: &mut PaxosSharedContext,
    ) -> Result<PaxosState, Box<dyn Error + Send + Sync>> {
        match event {
            PaxosEvent::AcceptResponseReceived(response) => {
                self.handle_accept_response(response, ctx).await?;
                Ok(PaxosState::Leader(self))
            }
            _ => Ok(PaxosState::Leader(self)),
        }
    }
}
