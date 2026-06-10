use crate::models::{LogEntry, LogPosition};
use crate::roles::{Follower, Leader, PaxosRole, util};
use crate::state_machine::PaxosState;
use crate::{
    ClientWriteResponse, PaxosEvent, PaxosSharedContext, PreVoteResponse, VoteOutcome,
    VoteRejection, VoteResponse,
};
use chrono::{DateTime, Utc};
use spx_lib::count_down_clock::CountDownClock;
use spx_lib::true_time::TrueTime;
use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use tonic::async_trait;
use uuid::Uuid;

pub struct Candidate {
    // Map of member IDs to their vote responses
    vote_board: HashMap<Uuid, Option<VoteResponse>>,

    // Map of member ID to their WAL log positions, carried over to Leader on election win
    score_board: HashMap<Uuid, LogPosition>,

    // The time at which dispatch_vote was called; guaranteed to be <= any individual
    // per-member dispatch time, so it is a safe conservative base for the leader lease
    vote_dispatch_time: Option<DateTime<Utc>>,

    // A count-down clock that fires VoteCampaignExpired after a fixed delay
    vote_cd_clock: CountDownClock,

    // Uncommitted log entries collected from vote promises; flushed to ctx only on quorum
    pending_uncommitted_logs: BTreeMap<u32, LogEntry>,
}

impl Candidate {
    pub fn new(ctx: &PaxosSharedContext) -> Self {
        let next_slot = ctx.get_last_log_slot() + 1;
        let vote_board = ctx.get_peer_ids().iter().map(|id| (*id, None)).collect();
        let score_board = ctx
            .get_peer_ids()
            .iter()
            .map(|id| (*id, LogPosition::with_next_slot(next_slot)))
            .collect();
        Self {
            vote_board,
            score_board,
            vote_dispatch_time: None,
            vote_cd_clock: CountDownClock::new(),
            pending_uncommitted_logs: BTreeMap::new(),
        }
    }

    pub async fn dispatch_vote(
        &mut self,
        ctx: &PaxosSharedContext,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Record before dispatch — this time is guaranteed to be <= any per-member send time
        self.vote_dispatch_time = Some(TrueTime::now().earliest);

        // Dispatch vote requests to other members to start the vote campaign
        let request = ctx.create_vote();
        ctx.get_dispatcher().dispatch_vote_request(request).await?;

        // Spawn a background task that fires VoteCampaignExpired after 3 seconds if not completed
        let event_tx = ctx.get_event_sender();
        self.vote_cd_clock.start_fixed(3000, move || {
            if let Err(e) = event_tx.try_send(PaxosEvent::VoteCampaignExpired) {
                eprintln!("Error: Failed to send VoteCampaignExpired event: {e}");
            }
        });
        Ok(())
    }

    fn handle_vote_request(&self, ctx: &PaxosSharedContext) -> VoteResponse {
        VoteResponse {
            member_id: ctx.get_current_member_id(),
            term: ctx.get_current_term(),
            outcome: VoteOutcome::Rejection(VoteRejection {
                current_leader_id: None,
                member_last_log_term: None,
                member_last_log_slot: None,
                // Already voted for self in this term
                voted_for_id: Some(ctx.get_current_member_id()),
            }),
        }
    }

    fn handle_pre_vote_response(&self, response: PreVoteResponse, ctx: &PaxosSharedContext) {
        if response.vote_granted {
            println!(
                "{} Info: Member {} granted pre-vote at term {}",
                ctx.log_prefix("Candidate"),
                response.member_id,
                response.term
            );
        } else {
            println!(
                "{} Info: Member {} rejected pre-vote: {}",
                ctx.log_prefix("Candidate"),
                response.member_id,
                response.rejection_reason()
            );
        }
    }

    async fn handle_vote_response(
        &mut self,
        response: VoteResponse,
        ctx: &mut PaxosSharedContext,
    ) -> Result<Option<Result<Leader, Follower>>, Box<dyn Error + Send + Sync>> {
        let member_id = response.member_id;

        // Log the rejection reason before recording the response
        if let VoteOutcome::Rejection(rejection) = &response.outcome {
            println!(
                "{} Info: Member {} rejected vote: {}",
                ctx.log_prefix("Candidate"),
                member_id,
                rejection.rejection_reason(response.term)
            );
        }

        // Record the response in the vote board
        self.vote_board.insert(member_id, Some(response.clone()));

        if let VoteOutcome::Promise(promise) = &response.outcome {
            if promise.last_log_term == ctx.get_last_log_term() {
                self.score_board
                    .get_mut(&member_id)
                    .expect("member must be in score board")
                    .match_slot = promise.last_log_slot.min(ctx.get_last_log_slot());
            }

            self.merge_uncommitted_logs(&promise.uncommitted_entries);
        }

        // Check if a quorum of members has granted votes
        if self.has_vote_quorum() {
            // Compute the leader lease expiry as the earliest vote dispatch time across the quorum
            // plus the lease length, this is guaranteed to be <= any quorum follower's vote lease expiry
            let lease_expiry = self.compute_leader_lease_expiry(ctx);
            ctx.update_leader_lease_expiry_time(lease_expiry).await;

            if !TrueTime::before(lease_expiry) {
                println!(
                    "{} Warning: Leader lease already expired by the time quorum was reached, stepping down as follower",
                    ctx.log_prefix("Candidate")
                );
                return Ok(Some(Err(Follower::new(None))));
            }

            println!(
                "{} Info: A quorum of votes has been granted, transitioning to leader",
                ctx.log_prefix("Candidate")
            );

            // Transition to a leader with the pre-built score board
            let score_board = std::mem::take(&mut self.score_board);
            let leader = Leader::new(score_board, ctx);

            // Quorum reached: flush the collected uncommitted entries into ctx before processing
            self.flush_uncommitted_logs(ctx);
            let mut leader = leader;
            leader.process_uncommitted_logs(ctx).await?;
            return Ok(Some(Ok(leader)));
        }

        // Check if all responses have been received and quorum is not reached
        if self.has_all_vote_responses() {
            println!(
                "{} Warning: All members responded but quorum not reached, stepping down as a follower",
                ctx.log_prefix("Candidate")
            );
            return Ok(Some(Err(Follower::new(None))));
        }

        Ok(None)
    }

    fn compute_leader_lease_expiry(&self, ctx: &PaxosSharedContext) -> DateTime<Utc> {
        self.vote_dispatch_time
            .expect("vote_dispatch_time must be set before computing leader lease expiry")
            + ctx.get_lease_length()
    }

    fn has_all_vote_responses(&self) -> bool {
        self.vote_board.values().all(|v| v.is_some())
    }

    fn has_vote_quorum(&self) -> bool {
        let num_matched = self
            .vote_board
            .values()
            .filter(|v| matches!(v, Some(r) if matches!(r.outcome, VoteOutcome::Promise(_))))
            .count();

        // + 1 accounts for the candidate's implicit self-vote, which is not in the board
        (num_matched + 1) >= (self.vote_board.len() + 1) / 2 + 1
    }

    fn merge_uncommitted_logs(&mut self, entries: &[LogEntry]) {
        for entry in entries {
            // Keep the highest-term entry seen at each slot across all promise responses
            let should_insert = self
                .pending_uncommitted_logs
                .get(&entry.slot)
                .map_or(true, |existing| entry.term > existing.term);

            if should_insert {
                self.pending_uncommitted_logs.insert(entry.slot, entry.clone());
            }
        }
    }

    fn flush_uncommitted_logs(&self, ctx: &mut PaxosSharedContext) {
        let committed_slot = ctx.get_committed_slot();
        let ctx_logs = ctx.get_uncommitted_logs_mut();
        for (slot, entry) in &self.pending_uncommitted_logs {
            if *slot <= committed_slot {
                continue;
            }

            let should_insert = ctx_logs
                .get(slot)
                .map_or(true, |existing| entry.term > existing.term);

            if should_insert {
                ctx_logs.insert(*slot, entry.clone());
            }
        }
    }
}

#[async_trait]
impl PaxosRole for Candidate {
    async fn handle_event(
        mut self,
        event: PaxosEvent,
        ctx: &mut PaxosSharedContext,
    ) -> Result<PaxosState, Box<dyn Error + Send + Sync>> {
        match event {
            PaxosEvent::LeaderLeaseExpired
            | PaxosEvent::ElectionCountdownExpired
            | PaxosEvent::PreVoteCampaignExpired => {
                // Already in a leader election process, ignore these events
                Ok(PaxosState::Candidate(self))
            }
            PaxosEvent::VoteCampaignExpired => {
                println!(
                    "{} Warning: Vote campaign timed out, stepping down as follower",
                    ctx.log_prefix("Candidate")
                );
                Ok(PaxosState::Follower(Follower::new(None)))
            }
            PaxosEvent::PreVoteRequestReceived(command) => {
                let request = command.get_request();
                let response = util::handle_pre_vote_request(request, ctx);
                command.send(response)?;
                Ok(PaxosState::Candidate(self))
            }
            PaxosEvent::PreVoteResponseReceived(response) => {
                self.handle_pre_vote_response(response, ctx);
                Ok(PaxosState::Candidate(self))
            }
            PaxosEvent::VoteRequestReceived(vote_command) => {
                let response = self.handle_vote_request(ctx);
                vote_command.send(response)?;
                Ok(PaxosState::Candidate(self))
            }
            PaxosEvent::VoteResponseReceived(response) => {
                if let Some(result) = self.handle_vote_response(response, ctx).await? {
                    return match result {
                        Ok(leader) => Ok(PaxosState::Leader(leader)),
                        Err(follower) => Ok(PaxosState::Follower(follower)),
                    };
                }
                Ok(PaxosState::Candidate(self))
            }
            PaxosEvent::AcceptRequestReceived(_) | PaxosEvent::AcceptResponseReceived(_) => {
                // Candidate is in the middle of voting, ignore accept messages
                Ok(PaxosState::Candidate(self))
            }
            PaxosEvent::ClientWriteRequestReceived(command) => {
                let _ = command.send(ClientWriteResponse {
                    success: false,
                    error: Some("not the leader".to_string()),
                });
                Ok(PaxosState::Candidate(self))
            }
            PaxosEvent::WriteFlushTimerFired => Ok(PaxosState::Candidate(self)),
            PaxosEvent::AcceptTimeoutCheckFired => Ok(PaxosState::Candidate(self)),
            PaxosEvent::HeartbeatTimerFired => Ok(PaxosState::Candidate(self)),
        }
    }
}

impl Drop for Candidate {
    fn drop(&mut self) {
        // Explicitly cancel so the countdown task is always stopped when the candidate transitions.
        self.vote_cd_clock.cancel();
    }
}
