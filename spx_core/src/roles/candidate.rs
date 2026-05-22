use crate::roles::{Follower, Leader, LogPosition, PaxosRole, util};
use crate::state_machine::PaxosState;
use crate::{LogEntry, PaxosEvent, PaxosSharedContext, PreVoteResponse, VoteOutcome, VoteResponse};
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
        &self,
        ctx: Arc<PaxosSharedContext>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Dispatch vote requests to other members to start the vote campaign
        let request = ctx.create_vote();
        ctx.get_dispatcher().dispatch_vote_request(request).await?;

        // Set a deadline for the whole vote campaign to 3 seconds
        todo!()
    }

    fn handle_pre_vote_response(&self, response: PreVoteResponse) {
        if response.vote_granted {
            println!(
                "Info: Member {} granted pre-vote at term {}",
                response.member_id, response.term
            );
        } else {
            println!(
                "Info: Member {} rejected pre-vote: {}",
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

        // Return early on rejection and wait for other responses
        if let VoteOutcome::Rejection(rejection) = &response.outcome {
            println!(
                "Info: Member {} rejected vote: {}",
                member_id,
                rejection.rejection_reason(response.term)
            );
            return Ok(None);
        }

        // Record and process the promised vote
        self.vote_board.insert(member_id, Some(response.clone()));

        if let VoteOutcome::Promise(promise) = &response.outcome {
            // History fingerprint check
            let trusted_match_slot = if promise.last_log_term == ctx.get_last_log_term() {
                promise.last_log_slot
            } else {
                0 // History diverged. Force a full sync later.
            };
            self.score_board
                .insert(member_id, LogPosition::from_match_slot(trusted_match_slot));

            self.merge_uncommitted_entries(&promise.uncommitted_entries);
        }

        // Check if a quorum of members has granted votes
        if self.has_vote_quorum() {
            println!("Info: A quorum of votes has been granted, transitioning to leader");
            let score_board = std::mem::take(&mut self.score_board);
            return Ok(Some(Ok(Leader::from_candidate(score_board))));
        }

        // Check if the vote campaign has timed out
        if self.has_vote_campaign_timeout() {
            println!("Warning: Vote campaign timeout occurred, stepping down as a follower");
            return Ok(Some(Err(Follower::new(None))));
        }
        Ok(None)
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

    fn merge_uncommitted_entries(&self, entries: &[LogEntry]) {
        for entry in entries {
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
            PaxosEvent::LeaderLeaseExpired => {
                // The candidate is already in a leader election process, ignore leader lease expiration event
                Ok(PaxosState::Candidate(self))
            }
            PaxosEvent::PreVoteRequestReceived(command) => {
                let request = command.get_request();
                let response = util::handle_pre_vote_request(request, ctx.clone());
                command.send(response)?;
                Ok(PaxosState::Candidate(self))
            }
            PaxosEvent::PreVoteResponseReceived(response) => {
                self.handle_pre_vote_response(response);
                Ok(PaxosState::Candidate(self))
            }
            PaxosEvent::VoteRequestReceived(_) => {
                todo!()
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
