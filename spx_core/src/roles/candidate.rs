use crate::models::{LogEntry, LogPosition};
use crate::roles::{Follower, Leader, PaxosRole, util};
use crate::state_machine::PaxosState;
use crate::{
    PaxosEvent, PaxosSharedContext, PreVoteResponse, VoteOutcome, VoteRejection, VoteResponse,
};
use chrono::{DateTime, Utc};
use dashmap::{DashMap, DashSet};
use spx_lib::count_down_clock::CountDownClock;
use spx_lib::true_time::TrueTime;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tonic::async_trait;
use uuid::Uuid;

pub struct Candidate {
    // A concurrent map of member IDs to their vote responses
    vote_board: DashMap<Uuid, Option<VoteResponse>>,

    // Map of member ID to their WAL log positions, carried over to Leader on election win
    score_board: DashMap<Uuid, LogPosition>,

    // Map of log slot to the winning uncommitted entry, resolved by highest term
    merged_uncommit_logs: DashMap<u32, LogEntry>,

    // The time at which the vote request was dispatched to each member
    vote_dispatch_times: Arc<DashMap<Uuid, DateTime<Utc>>>,

    // A count-down clock that fires VoteCampaignExpired after a fixed delay
    vote_cd_clock: CountDownClock,
}

impl Candidate {
    pub fn new(peer_ids: &DashSet<Uuid>, event_tx: Sender<PaxosEvent>) -> Self {
        let vote_board = DashMap::new();
        for peer_id in peer_ids.iter() {
            vote_board.insert(*peer_id, None);
        }
        let vote_cd_clock = CountDownClock::new(move || {
            if let Err(e) = event_tx.try_send(PaxosEvent::VoteCampaignExpired) {
                eprintln!("Error: Failed to send VoteCampaignExpired event: {e}");
            }
        });
        Self {
            vote_board,
            score_board: DashMap::new(),
            merged_uncommit_logs: DashMap::new(),
            vote_dispatch_times: Arc::new(DashMap::new()),
            vote_cd_clock,
        }
    }

    pub async fn dispatch_vote(
        &mut self,
        ctx: Arc<PaxosSharedContext>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Dispatch vote requests to other members to start the vote campaign
        let request = ctx.create_vote();
        let vote_dispatch_times = self.vote_dispatch_times.clone();
        ctx.get_dispatcher()
            .dispatch_vote_request(
                request,
                Arc::new(move |member_id| {
                    vote_dispatch_times.insert(member_id, TrueTime::now().earliest);
                }),
            )
            .await?;

        // Spawn a background task that fires VoteCampaignExpired after 3 seconds if not completed
        self.vote_cd_clock.start_fixed(3000);
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
        ctx: Arc<PaxosSharedContext>,
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
            let trusted_match_slot = if promise.last_log_term == ctx.get_last_log_term() {
                promise.last_log_slot
            } else {
                // The term of the last log from the promise is less than local last log term,
                // The member might be writing logs at a stale term and would require a full log re-sync process when a leader is elected
                0
            };

            self.score_board
                .insert(member_id, LogPosition::from_match_slot(trusted_match_slot));

            self.merge_uncommitted_logs(&promise.uncommitted_entries);
        }

        // Check if a quorum of members has granted votes
        if self.has_vote_quorum() {
            // Compute the leader lease expiry as the earliest vote dispatch time across the quorum
            // plus the lease length — this is guaranteed to be <= any quorum follower's vote lease expiry
            let lease_expiry = self.compute_leader_lease_expiry(&ctx);
            ctx.update_leader_lease_expiry_time(lease_expiry).await;

            if !TrueTime::before(lease_expiry) {
                println!(
                    "{} Warning: Leader lease already expired by the time quorum was reached, stepping down as follower",
                    ctx.log_prefix("Candidate")
                );
                return Ok(Some(Err(Follower::new(None, ctx.get_event_sender()))));
            }

            println!(
                "{} Info: A quorum of votes has been granted, transitioning to leader",
                ctx.log_prefix("Candidate")
            );

            // Transition to a leader with a pre-constructed score board
            let score_board = std::mem::take(&mut self.score_board);
            let leader = Leader::new(score_board);

            // Merge local uncommitted entries into the candidate's collected logs, keeping the highest-term entry at each slot
            let local_uncommitted = ctx.get_uncommitted_entries().await;
            self.merge_uncommitted_logs(&local_uncommitted);
            let uncommitted_logs: Vec<LogEntry> = std::mem::take(&mut self.merged_uncommit_logs)
                .into_iter()
                .map(|(_, entry)| entry)
                .collect();

            // Process the uncommitted logs found locally and reported by other members
            leader.process_uncommitted_logs(uncommitted_logs);
            return Ok(Some(Ok(leader)));
        }

        // Check if all responses have been received and quorum is not reached
        if self.has_all_vote_responses() {
            println!(
                "{} Warning: All members responded but quorum not reached, stepping down as a follower",
                ctx.log_prefix("Candidate")
            );
            return Ok(Some(Err(Follower::new(None, ctx.get_event_sender()))));
        }

        Ok(None)
    }

    // Returns the earliest dispatch time among quorum members that granted a vote promise,
    // used to compute a conservative leader lease expiry
    fn get_min_dispatch_time(&self) -> Option<DateTime<Utc>> {
        self.vote_board
            .iter()
            .filter(|e| matches!(e.value(), Some(r) if matches!(r.outcome, VoteOutcome::Promise(_))))
            .filter_map(|e| self.vote_dispatch_times.get(e.key()).map(|t| *t))
            .min()
    }

    fn compute_leader_lease_expiry(&self, ctx: &PaxosSharedContext) -> DateTime<Utc> {
        let min_time = self
            .get_min_dispatch_time()
            .expect("vote dispatch times must be populated before computing leader lease expiry");
        min_time + ctx.get_lease_length()
    }

    fn has_all_vote_responses(&self) -> bool {
        self.vote_board.iter().all(|entry| entry.value().is_some())
    }

    fn has_vote_quorum(&self) -> bool {
        let num_matched = self
            .vote_board
            .iter()
            .filter(|entry| matches!(entry.value(), Some(r) if matches!(r.outcome, VoteOutcome::Promise(_))))
            .count();

        // + 1 accounts for the candidate's implicit self-vote, which is not in the board
        (num_matched + 1) >= (self.vote_board.len() + 1) / 2 + 1
    }

    fn merge_uncommitted_logs(&self, entries: &[LogEntry]) {
        for entry in entries {
            // Only include the uncommited log at a specific slot if its term number is higher than any other
            // term number reported at the same slot
            let should_insert = self
                .merged_uncommit_logs
                .get(&entry.slot)
                .map_or(true, |existing| entry.term > existing.term);

            if should_insert {
                self.merged_uncommit_logs.insert(entry.slot, entry.clone());
            }
        }
    }
}

#[async_trait]
impl PaxosRole for Candidate {
    async fn handle_event(
        mut self,
        event: PaxosEvent,
        ctx: Arc<PaxosSharedContext>,
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
                Ok(PaxosState::Follower(Follower::new(
                    None,
                    ctx.get_event_sender(),
                )))
            }
            PaxosEvent::PreVoteRequestReceived(command) => {
                let request = command.get_request();
                let response = util::handle_pre_vote_request(request, ctx.clone());
                command.send(response)?;
                Ok(PaxosState::Candidate(self))
            }
            PaxosEvent::PreVoteResponseReceived(response) => {
                self.handle_pre_vote_response(response, &ctx);
                Ok(PaxosState::Candidate(self))
            }
            PaxosEvent::VoteRequestReceived(vote_command) => {
                let response = self.handle_vote_request(&ctx);
                vote_command.send(response)?;
                Ok(PaxosState::Candidate(self))
            }
            PaxosEvent::VoteResponseReceived(response) => {
                if let Some(result) = self.handle_vote_response(response, ctx.clone()).await? {
                    return match result {
                        Ok(leader) => Ok(PaxosState::Leader(leader)),
                        Err(follower) => Ok(PaxosState::Follower(follower)),
                    };
                }
                Ok(PaxosState::Candidate(self))
            }
            PaxosEvent::ReplicateWriteRequestReceived(_command) => {
                // Candidate is in the middle of voting, ignore replicate write requests
                Ok(PaxosState::Candidate(self))
            }
            PaxosEvent::ReplicateWriteResponseReceived(_response) => {
                // Candidate is in the middle of voting, ignore replicate write responses
                Ok(PaxosState::Candidate(self))
            }
        }
    }
}

impl Drop for Candidate {
    fn drop(&mut self) {
        // Explicitly cancel so the countdown task is always stopped when the candidate transitions.
        self.vote_cd_clock.cancel();
    }
}
