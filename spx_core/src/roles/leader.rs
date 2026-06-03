use crate::models::{LogEntry, LogPosition};
use crate::roles::PaxosRole;
use crate::state_machine::PaxosState;
use crate::{
    AcceptLogEntry, AcceptRequest, AcceptResponse, ClientWriteRequest, ClientWriteResponse,
    PaxosCommand, PaxosEvent, PaxosSharedContext,
};
use chrono::{DateTime, Utc};
use spx_lib::true_time::TrueTime;
use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use tonic::async_trait;
use uuid::Uuid;

// The state of Paxos group leader
pub struct Leader {
    // A map of Paxos group member IDs to their local WAL positions
    score_board: HashMap<Uuid, LogPosition>,

    // Tracks the most recent echoed t_send per follower; used for quorum lease renewal
    last_contact: HashMap<Uuid, DateTime<Utc>>,

    // In-flight client write commands awaiting quorum commit, keyed by slot number
    in_flight_writes: BTreeMap<u32, PaxosCommand<ClientWriteRequest, ClientWriteResponse>>,
}

impl Leader {
    pub fn new(score_board: HashMap<Uuid, LogPosition>) -> Self {
        let last_contact = score_board
            .keys()
            .copied()
            .map(|id| (id, DateTime::<Utc>::UNIX_EPOCH))
            .collect();
        Self {
            score_board,
            last_contact,
            in_flight_writes: BTreeMap::new(),
        }
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
        &mut self,
        ctx: &PaxosSharedContext,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let member_ids: Vec<Uuid> = self.score_board.keys().copied().collect();
        for member_id in member_ids {
            self.dispatch_accept_request_to_member(member_id, ctx)
                .await?;
        }
        Ok(())
    }

    async fn dispatch_accept_request_to_member(
        &mut self,
        member_id: Uuid,
        ctx: &PaxosSharedContext,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let pos = self
            .score_board
            .get(&member_id)
            .expect("dispatch target must be in the score board");

        let wal = ctx.get_wal();
        let sent_slot = pos.sent_slot;
        let prev_log_slot = if sent_slot > pos.next_slot {
            sent_slot
        } else {
            pos.next_slot.saturating_sub(1)
        };
        let prev_log_term = if prev_log_slot == 0 {
            0
        } else {
            wal.get_term(prev_log_slot)
                .expect("prev_log_slot is a slot previously sent, so it must be in the WAL")
        };

        let entries: Vec<AcceptLogEntry> = wal
            .get_entries_from(prev_log_slot + 1)
            .into_iter()
            .map(|(slot, term, entry)| AcceptLogEntry { term, slot, entry })
            .collect();

        let last_sent_slot = entries.last().map(|e| e.slot);

        let request = AcceptRequest {
            term: ctx.get_current_term(),
            leader_id: ctx.get_current_member_id(),
            prev_log_slot,
            prev_log_term,
            entries,
            leader_commit_slot: ctx.get_committed_slot(),
            t_send: TrueTime::now().earliest,
        };

        ctx.get_dispatcher()
            .dispatch_accept_request(member_id, request)
            .await?;

        if let Some(slot) = last_sent_slot {
            self.score_board
                .get_mut(&member_id)
                .expect("dispatch target must be in the score board")
                .sent_slot = slot;
        }

        Ok(())
    }

    fn resolve_committed_writes(&mut self, committed_slot: u32) {
        let still_in_flight = self.in_flight_writes.split_off(&(committed_slot + 1));
        let committed = std::mem::replace(&mut self.in_flight_writes, still_in_flight);
        for (_, cmd) in committed {
            // Ignore send errors — the client may have disconnected before the write committed,
            // which is not a leader-level failure
            let _ = cmd.send(ClientWriteResponse { success: true, error: None });
        }
    }

    async fn handle_client_write(
        &mut self,
        command: PaxosCommand<ClientWriteRequest, ClientWriteResponse>,
        ctx: &mut PaxosSharedContext,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let current_term = ctx.get_current_term();
        let slot = ctx.get_last_log_slot() + 1;
        let value = command.get_request().value;

        ctx.get_wal_mut().append(slot, current_term, value.clone());
        ctx.set_last_log_slot(slot);
        ctx.set_last_log_term(current_term);
        ctx.get_uncommitted_logs_mut().insert(slot, LogEntry {
            term: current_term,
            slot,
            entry: value,
        });

        self.dispatch_accept_request(ctx).await?;

        self.in_flight_writes.insert(slot, command);
        Ok(())
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

        let quorum_size = ctx.get_quorum_size();

        // Update last_contact of the follower to the max of the stored and echoed t_send values
        self.last_contact
            .entry(member_id)
            .and_modify(|t| *t = (*t).max(response.echoed_t_send))
            .or_insert(response.echoed_t_send);

        // Check if a quorum of followers has been in recent contact; if so, extend the lease.
        let mut contact_times: Vec<DateTime<Utc>> = self.last_contact.values().copied().collect();
        contact_times.sort_unstable_by(|a, b| b.cmp(a)); // newest-first

        // Index quorum_size-2: contact_times is followers-only; the leader counts itself,
        // so only quorum_size-1 followers are needed. The weakest of those is at that index.
        if let Some(&t_quorum) = contact_times.get(quorum_size - 2) {
            let proposed_expiry = t_quorum + ctx.get_lease_length();
            let current_expiry = ctx.get_leader_lease_expiry().await;

            // Only advance the lease; never shorten it.
            if current_expiry.map_or(true, |current| proposed_expiry > current) {
                ctx.update_leader_lease_expiry_time(proposed_expiry).await;
            }
        }

        if response.success {
            // Follower successfully appended — advance its position in the score board
            let last_written_slot = response.last_written_slot;
            let pos = self
                .score_board
                .get_mut(&member_id)
                .expect("accept response received from unknown member");

            if last_written_slot > pos.match_slot {
                pos.match_slot = last_written_slot;
                pos.next_slot = last_written_slot + 1;
                if pos.sent_slot < pos.match_slot {
                    pos.sent_slot = pos.match_slot;
                }

                // The follower's match_slot advanced; recheck whether a quorum has replicated
                // enough to advance the committed slot.
                let mut follower_slots: Vec<u32> =
                    self.score_board.values().map(|p| p.match_slot).collect();
                follower_slots.sort_unstable_by(|a, b| b.cmp(a));

                // Gather the top (quorum_size - 1) follower match_slots plus the leader's own
                // last_log_slot, then take the minimum — the highest slot guaranteed on a quorum.
                let min_quorum_slot = follower_slots
                    .into_iter()
                    .take(quorum_size - 1)
                    .chain(std::iter::once(ctx.get_last_log_slot()))
                    .min()
                    .unwrap_or(0);

                if min_quorum_slot > ctx.get_committed_slot() {
                    ctx.set_committed_slot(min_quorum_slot);
                    ctx.get_uncommitted_logs_mut()
                        .retain(|&slot, _| slot > min_quorum_slot);
                    self.resolve_committed_writes(min_quorum_slot);
                }
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
        pos.sent_slot = pos.next_slot.saturating_sub(1);

        self.dispatch_accept_request_to_member(member_id, ctx).await
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
            PaxosEvent::ClientWriteRequestReceived(command) => {
                self.handle_client_write(command, ctx).await?;
                Ok(PaxosState::Leader(self))
            }
            _ => Ok(PaxosState::Leader(self)),
        }
    }
}
