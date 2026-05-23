use crate::models::{LogEntry, LogPosition};
use crate::roles::{Follower, Leader, PaxosRole, util};
use crate::state_machine::PaxosState;
use crate::{PaxosEvent, PaxosSharedContext, PreVoteResponse, VoteOutcome, VoteRejection, VoteResponse};
use dashmap::{DashMap, DashSet};
use std::error::Error;
use std::sync::Arc;
use tokio::time;
use tonic::async_trait;
use uuid::Uuid;

pub struct Candidate {
    // A concurrent map of member IDs to their vote responses
    vote_board: DashMap<Uuid, Option<VoteResponse>>,

    // Map of member ID to their WAL log positions, carried over to Leader on election win
    score_board: DashMap<Uuid, LogPosition>,

    // Map of log slot to the winning uncommitted entry, resolved by highest term
    uncommitted_logs: DashMap<u32, LogEntry>,

    // The deadline for the vote campaign
    vote_campaign_deadline: Option<time::Instant>,
}

impl Candidate {
    pub fn new(peer_ids: &DashSet<Uuid>) -> Self {
        let vote_board = DashMap::new();
        for peer_id in peer_ids.iter() {
            vote_board.insert(*peer_id, None);
        }
        Self {
            vote_board,
            score_board: DashMap::new(),
            uncommitted_logs: DashMap::new(),
            vote_campaign_deadline: None,
        }
    }

    pub async fn dispatch_vote(
        &mut self,
        ctx: Arc<PaxosSharedContext>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Dispatch vote requests to other members to start the vote campaign
        let request = ctx.create_vote();
        ctx.get_dispatcher().dispatch_vote_request(request).await?;

        // Set a deadline for the whole vote campaign to 3 seconds
        self.vote_campaign_deadline =
            Some(time::Instant::now() + time::Duration::from_millis(3000));
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
                "[Candidate {}] Info: Member {} granted pre-vote at term {}",
                ctx.get_current_member_id(), response.member_id, response.term
            );
        } else {
            println!(
                "[Candidate {}] Info: Member {} rejected pre-vote: {}",
                ctx.get_current_member_id(),
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
                "[Candidate {}] Info: Member {} rejected vote: {}",
                ctx.get_current_member_id(),
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
            println!("[Candidate {}] Info: A quorum of votes has been granted, transitioning to leader", ctx.get_current_member_id());
            let score_board = std::mem::take(&mut self.score_board);
            return Ok(Some(Ok(Leader::from_candidate(score_board))));
        }

        // Check if all responses have been received and quorum is not reached
        if self.has_all_vote_responses() {
            println!("[Candidate {}] Warning: All members responded but quorum not reached, stepping down as a follower", ctx.get_current_member_id());
            return Ok(Some(Err(Follower::new(None, ctx.get_event_sender()))));
        }

        // Check if the vote campaign has timed out
        if self.has_vote_campaign_timeout() {
            println!("[Candidate {}] Warning: Vote campaign timeout occurred, stepping down as a follower", ctx.get_current_member_id());
            return Ok(Some(Err(Follower::new(None, ctx.get_event_sender()))));
        }
        Ok(None)
    }

    fn has_all_vote_responses(&self) -> bool {
        self.vote_board.iter().all(|entry| entry.value().is_some())
    }

    fn has_vote_quorum(&self) -> bool {
        let num_matched = self
            .vote_board
            .iter()
            .filter(|entry| matches!(entry.value(), Some(r) if matches!(r.outcome, crate::VoteOutcome::Promise(_))))
            .count();

        // + 1 accounts for the candidate's implicit self-vote, which is not in the board
        (num_matched + 1) >= (self.vote_board.len() + 1) / 2 + 1
    }

    fn has_vote_campaign_timeout(&self) -> bool {
        self.vote_campaign_deadline.is_some()
            && time::Instant::now() > self.vote_campaign_deadline.unwrap()
    }

    fn merge_uncommitted_logs(&self, entries: &[LogEntry]) {
        for entry in entries {
            // Only include the uncommited log at a specific slot if its term number is higher than any other
            // term number reported at the same slot
            let should_insert = self
                .uncommitted_logs
                .get(&entry.slot)
                .map_or(true, |existing| entry.term > existing.term);

            if should_insert {
                self.uncommitted_logs.insert(entry.slot, entry.clone());
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
            PaxosEvent::LeaderLeaseExpired | PaxosEvent::ElectionCountdownExpired => {
                // Already in a leader election process, ignore these events
                Ok(PaxosState::Candidate(self))
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
        }
    }
}
