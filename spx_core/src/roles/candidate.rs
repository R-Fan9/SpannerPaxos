use crate::roles::{Follower, Leader, PaxosRole, util};
use crate::state_machine::PaxosState;
use crate::{LogEntry, PaxosEvent, PaxosSharedContext, PreVoteResponse, VoteOutcome, VoteResponse};
use dashmap::{DashMap, DashSet};
use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use tokio::time;
use tonic::async_trait;
use uuid::Uuid;

pub struct Candidate {
    // A concurrent map of member IDs to their vote responses
    vote_board: DashMap<Uuid, Option<VoteResponse>>,

    // Map of member ID to their last known log slot (match index)
    future_match_index: HashMap<Uuid, u32>,

    // Map of log slot to (term, entry) for resolved uncommitted entries
    resolved_uncommitted_logs: HashMap<u32, (u32, LogEntry)>,

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
            future_match_index: HashMap::new(),
            resolved_uncommitted_logs: HashMap::new(),
            vote_campaign_deadline: None,
        }
    }

    pub fn has_vote_quorum(&self) -> bool {
        let num_matched = self
            .vote_board
            .iter()
            .filter(|entry| matches!(entry.value(), Some(r) if matches!(r.outcome, crate::VoteOutcome::Promise(_))))
            .count();

        // + 1 accounts for the candidate's implicit self-vote, which is not in the board
        (num_matched + 1) >= (self.vote_board.len() + 1) / 2 + 1
    }

    pub fn has_vote_rejected(&self) -> bool {
        let num_matched = self
            .vote_board
            .iter()
            .filter(|entry| matches!(entry.value(), Some(r) if matches!(r.outcome, crate::VoteOutcome::Rejection(_))))
            .count();

        // + 1 account for the candidate, which is not in the board
        num_matched >= (self.vote_board.len() + 1) / 2 + 1
    }

    pub fn has_vote_campaign_timeout(&self) -> bool {
        self.vote_campaign_deadline.is_some()
            && time::Instant::now() > self.vote_campaign_deadline.unwrap()
    }

    async fn handle_vote_response(
        &mut self,
        response: VoteResponse,
        ctx: Arc<PaxosSharedContext>,
    ) -> Result<Option<Result<Leader, Follower>>, Box<dyn Error + Send + Sync>> {
        let member_id = response.member_id;

        // Step down as a follower if the response indicates an active leader with a term strictly greater than the local term
        if let Some(leader_id) = response.try_get_leader(|received_term| received_term > ctx.get_current_term()) {
            println!(
                "Info: Member {} reported active leader {} at term {}, stepping down as a follower",
                member_id, leader_id, response.term
            );
            return Ok(Some(Err(Follower::new(Some(leader_id)))));
        }

        // Record the vote response
        self.vote_board.insert(member_id, Some(response.clone()));

        // Process the response to update match index and uncommitted logs
        if let crate::VoteOutcome::Promise(promise) = &response.outcome {
            // History fingerprint check
            let trusted_match_slot = if promise.last_log_term == ctx.get_last_log_term() {
                promise.last_log_slot
            } else {
                0 // History diverged. Force a full sync later.
            };
            self.future_match_index.insert(member_id, trusted_match_slot);

            // Merge uncommitted conflicts ("Highest Term Wins")
            for entry in &promise.uncommitted_entries {
                if let Some((existing_term, _)) = self.resolved_uncommitted_logs.get(&entry.slot) {
                    if entry.term > *existing_term {
                        self.resolved_uncommitted_logs.insert(entry.slot, (entry.term, entry.clone()));
                    }
                } else {
                    self.resolved_uncommitted_logs.insert(entry.slot, (entry.term, entry.clone()));
                }
            }
        }

        // Check if a quorum of members has granted votes
        if self.has_vote_quorum() {
            println!("Info: A quorum of votes has been granted, transitioning to leader");
            return Ok(Some(Ok(Leader::new())));
        }

        // Check if a quorum of members has rejected votes or the vote campaign has timed out
        if self.has_vote_rejected() || self.has_vote_campaign_timeout() {
            println!("Warning: A quorum of votes has been rejected or timeout occurred, stepping down as a follower");
            return Ok(Some(Err(Follower::new(None))));
        }
        Ok(None)
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

    fn handle_pre_vote_response(
        &self,
        response: PreVoteResponse,
        ctx: &PaxosSharedContext,
    ) -> Option<Follower> {
        let leader_id = response.try_get_leader(|received_term| received_term > ctx.get_current_term())?;

        // Step down as a follower if the response indicates an active leader with a term strictly greater than the local term
        println!(
            "Info: Member {} reported active leader {} at term {}, stepping down as a follower",
            response.member_id, leader_id, response.term
        );
        Some(Follower::new(Some(leader_id)))
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
                if let Some(follower) = self.handle_pre_vote_response(response, &ctx) {
                    return Ok(PaxosState::Follower(follower));
                }
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
