use crate::models::{LogEntry, LogPosition};
use crate::roles::PaxosRole;
use crate::state_machine::PaxosState;
use crate::{
    AcceptLogEntry, AcceptRequest, AcceptResponse, ClientWriteRequest, ClientWriteResponse,
    PaxosCommand, PaxosEvent, PaxosSharedContext,
};
use chrono::{DateTime, Utc};
use spx_lib::true_time::TrueTime;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::error::Error;
use std::time::Duration;
use tonic::async_trait;
use uuid::Uuid;

struct InFlightBatch {
    sent_at: DateTime<Utc>,
    // The highest log slot included in this batch
    highest_slot: u32,
}

struct FollowerState {
    log_position: LogPosition,
    // FIFO queue of batches dispatched to this follower but not yet acknowledged
    in_flight_batches: VecDeque<InFlightBatch>,
}

impl FollowerState {
    fn new(log_position: LogPosition) -> Self {
        Self {
            log_position,
            in_flight_batches: VecDeque::new(),
        }
    }

    fn push_batch(&mut self, highest_slot: u32, sent_at: DateTime<Utc>) {
        self.in_flight_batches.push_back(InFlightBatch {
            sent_at,
            highest_slot,
        });
    }

    // Clears all batches whose highest_slot is at or below `acked_slot`.
    fn ack_batches_up_to(&mut self, acked_slot: u32) {
        while self
            .in_flight_batches
            .front()
            .is_some_and(|b| b.highest_slot <= acked_slot)
        {
            self.in_flight_batches.pop_front();
        }
    }

    // Clears all in-flight batches and resets sent_slot to just before next_slot,
    // used when a rejection is received or a batch times out.
    fn clear_in_flight_batches(&mut self) {
        self.in_flight_batches.clear();
        self.log_position.sent_slot = self.log_position.next_slot.saturating_sub(1);
    }
}

// The state of Paxos group leader
pub struct Leader {
    // Per-follower replication state and in-flight batch tracking
    score_board: HashMap<Uuid, FollowerState>,

    // Tracks the most recent echoed t_send per follower; used for quorum lease renewal
    last_contact: HashMap<Uuid, DateTime<Utc>>,

    // In-flight client write requests awaiting quorum commit, keyed by slot number
    in_flight_writes: BTreeMap<u32, PaxosCommand<ClientWriteRequest, ClientWriteResponse>>,

    // Number of client writes buffered since the last dispatch_accept_request call
    pending_write_count: usize,

    // Dispatch is triggered when this many writes are buffered, even if the timer has not fired
    write_batch_size: usize,
}

impl Leader {
    pub const DEFAULT_WRITE_BATCH_SIZE: usize = 1000;
    pub const DEFAULT_WRITE_FLUSH_INTERVAL: Duration = Duration::from_millis(10);
    const ACCEPT_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
    const ACCEPT_TIMEOUT_CHECK_INTERVAL: Duration = Duration::from_secs(60);

    fn spawn_write_flush_timer(ctx: &PaxosSharedContext) {
        let token = ctx.get_cancellation_token();
        let signal = ctx.signal_write_flush_fn();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => break,
                    _ = tokio::time::sleep(Self::DEFAULT_WRITE_FLUSH_INTERVAL) => signal(),
                }
            }
        });
    }

    fn spawn_accept_timeout_check_timer(ctx: &PaxosSharedContext) {
        let token = ctx.get_cancellation_token();
        let signal = ctx.signal_accept_timeout_check_fn();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => break,
                    _ = tokio::time::sleep(Self::ACCEPT_TIMEOUT_CHECK_INTERVAL) => signal(),
                }
            }
        });
    }

    pub fn new(score_board: HashMap<Uuid, LogPosition>, ctx: &PaxosSharedContext) -> Self {
        let last_contact = score_board
            .keys()
            .copied()
            .map(|id| (id, DateTime::<Utc>::UNIX_EPOCH))
            .collect();

        let score_board = score_board
            .into_iter()
            .map(|(id, pos)| (id, FollowerState::new(pos)))
            .collect();

        Self::spawn_write_flush_timer(ctx);
        Self::spawn_accept_timeout_check_timer(ctx);

        Self {
            score_board,
            last_contact,
            in_flight_writes: BTreeMap::new(),
            pending_write_count: 0,
            write_batch_size: Self::DEFAULT_WRITE_BATCH_SIZE,
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
            for fs in self.score_board.values_mut() {
                fs.log_position.next_slot = next_slot;
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
            self.dispatch_accept_request_to_follower(member_id, ctx)
                .await?;
        }
        Ok(())
    }

    async fn dispatch_accept_request_to_follower(
        &mut self,
        member_id: Uuid,
        ctx: &PaxosSharedContext,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let fs = self
            .score_board
            .get(&member_id)
            .expect("dispatch target must be in the score board");

        let pos = &fs.log_position;
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
        let t_send = TrueTime::now().earliest;

        let request = AcceptRequest {
            term: ctx.get_current_term(),
            leader_id: ctx.get_current_member_id(),
            prev_log_slot,
            prev_log_term,
            entries,
            leader_commit_slot: ctx.get_committed_slot(),
            t_send,
        };

        ctx.get_dispatcher()
            .dispatch_accept_request(member_id, request)
            .await?;

        if let Some(slot) = last_sent_slot {
            let fs = self
                .score_board
                .get_mut(&member_id)
                .expect("dispatch target must be in the score board");
            fs.log_position.sent_slot = slot;
            fs.push_batch(slot, t_send);
        }

        Ok(())
    }

    // Responds to all pending client write requests at or below `committed_slot`, where
    // `committed_slot` is the highest slot at which a quorum of followers has persisted the log.
    fn resolve_in_flight_writes(&mut self, committed_slot: u32) {
        let still_in_flight = self.in_flight_writes.split_off(&(committed_slot + 1));
        let committed = std::mem::replace(&mut self.in_flight_writes, still_in_flight);
        for (_, cmd) in committed {
            // Ignore send errors — the client may have disconnected before the write committed,
            // which is not a leader-level failure
            let _ = cmd.send(ClientWriteResponse {
                success: true,
                error: None,
            });
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
        ctx.get_uncommitted_logs_mut().insert(
            slot,
            LogEntry {
                term: current_term,
                slot,
                entry: value,
            },
        );

        self.in_flight_writes.insert(slot, command);
        self.pending_write_count += 1;

        // If the batch is full, wake the flush watcher immediately so the state machine's
        // select arm fires without queuing behind other pending PaxosEvents.
        if self.pending_write_count >= self.write_batch_size {
            ctx.signal_write_flush();
        }

        Ok(())
    }

    async fn handle_write_flush(
        &mut self,
        ctx: &mut PaxosSharedContext,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        if self.pending_write_count == 0 {
            return Ok(());
        }
        self.dispatch_accept_request(ctx).await?;
        self.pending_write_count = 0;
        Ok(())
    }

    // Scans every follower's in-flight batch queue and clears batches whose oldest entry has
    // been waiting longer than ACCEPT_REQUEST_TIMEOUT, resetting that follower's sent_slot.
    fn handle_accept_timeout_check(&mut self) {
        let now = Utc::now();
        let timeout = chrono::Duration::from_std(Self::ACCEPT_REQUEST_TIMEOUT)
            .expect("timeout fits in chrono::Duration");

        for fs in self.score_board.values_mut() {
            if fs
                .in_flight_batches
                .front()
                .is_some_and(|b| now - b.sent_at > timeout)
            {
                fs.clear_in_flight_batches();
            }
        }
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
            let last_written_slot = response.last_written_slot;
            let fs = self
                .score_board
                .get_mut(&member_id)
                .expect("accept response received from unknown member");

            if last_written_slot > fs.log_position.match_slot {
                fs.log_position.match_slot = last_written_slot;
                fs.log_position.next_slot = last_written_slot + 1;
                if fs.log_position.sent_slot < fs.log_position.match_slot {
                    fs.log_position.sent_slot = fs.log_position.match_slot;
                }
                fs.ack_batches_up_to(last_written_slot);

                // The follower's match_slot advanced; recheck whether a quorum has replicated
                // enough to advance the committed slot.
                let mut follower_slots: Vec<u32> = self
                    .score_board
                    .values()
                    .map(|fs| fs.log_position.match_slot)
                    .collect();
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
                    self.resolve_in_flight_writes(min_quorum_slot);
                }
            }

            return Ok(());
        }

        let Some(hint) = response.conflict_hint else {
            return Ok(());
        };

        let fs = self
            .score_board
            .get_mut(&member_id)
            .expect("accept response received from unknown member");

        // Clear all in-flight batches for this follower — the rejection means our view of its
        // log is inconsistent and we need to re-probe from scratch.
        fs.clear_in_flight_batches();

        if hint.conflict_term == 0 {
            // Follower log is too short — back up to the first missing slot from the follower
            fs.log_position.next_slot = hint.conflict_first_slot;
        } else {
            // Term mismatch at anchor slot
            fs.log_position.next_slot =
                match ctx.get_wal().find_highest_slot_for_term(hint.conflict_term) {
                    // Leader has entries for this term — align just past its last slot
                    Some(leader_last_slot) => leader_last_slot + 1,
                    // Leader has no entries for this term — skip to follower's hint
                    None => hint.conflict_first_slot,
                };
        }

        // Dispatch another accept request to the follower with updated log anchor and entries
        self.dispatch_accept_request_to_follower(member_id, ctx).await
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
            PaxosEvent::WriteFlushTimerFired => {
                self.handle_write_flush(ctx).await?;
                Ok(PaxosState::Leader(self))
            }
            PaxosEvent::AcceptTimeoutCheckFired => {
                self.handle_accept_timeout_check();
                Ok(PaxosState::Leader(self))
            }
            _ => Ok(PaxosState::Leader(self)),
        }
    }
}
