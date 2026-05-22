use crate::roles::{Candidate, Follower, PaxosRole, util};
use crate::state_machine::PaxosState;
use crate::{PaxosEvent, PaxosSharedContext, PreVoteResponse};
use dashmap::{DashMap, DashSet};
use std::error::Error;
use std::sync::Arc;
use tokio::time;
use tonic::async_trait;
use uuid::Uuid;

// The state of Paxos group leader pre-candidate - Thread-safe without external locking
pub struct PreCandidate {
    // A concurrent map of Paxos group member IDs to their pre-vote responses
    pre_vote_board: DashMap<Uuid, Option<PreVoteResponse>>,

    // The deadline for the pre-vote campaign
    pre_vote_campaign_deadline: Option<time::Instant>,
}

impl PreCandidate {
    pub fn new(peer_ids: &DashSet<Uuid>) -> Self {
        let pre_vote_board = DashMap::new();
        for peer_id in peer_ids.iter() {
            pre_vote_board.insert(*peer_id, None);
        }
        Self {
            pre_vote_board,
            pre_vote_campaign_deadline: None,
        }
    }

    pub async fn dispatch_pre_vote(
        &mut self,
        ctx: Arc<PaxosSharedContext>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Dispatch pre-vote requests to other members to start the pre-vote campaign
        let request = ctx.create_pre_vote();
        ctx.get_dispatcher()
            .dispatch_prevote_request(request)
            .await?;

        // Set a deadline for the whole pre-vote campaign to 3 seconds
        self.pre_vote_campaign_deadline =
            Some(time::Instant::now() + time::Duration::from_millis(3000));
        Ok(())
    }

    async fn handle_pre_vote_response(
        &self,
        response: PreVoteResponse,
        ctx: Arc<PaxosSharedContext>,
    ) -> Result<Option<Result<Candidate, Follower>>, Box<dyn Error + Send + Sync>> {
        // Log the rejection reason and continue waiting for other responses
        if !response.vote_granted {
            println!(
                "Info: Member {} rejected pre-vote: {}",
                response.member_id,
                response.rejection_reason()
            );
            return Ok(None);
        }

        // Keep track of the granted pre-vote
        self.update_pre_vote_board(response);

        // Check if a quorum of members has granted the pre-vote
        if self.has_pre_vote_quorum() {
            println!(
                "Info: A quorum of pre-votes has been granted, transitioning to leader candidate"
            );

            // Transition to a leader candidate
            let candidate = Candidate::new(ctx.get_peer_ids());

            // Increment the current term number
            ctx.increment_current_term();

            // Dispatch vote requests to other members to grant votes to become a leader
            candidate.dispatch_vote(ctx).await?;
            return Ok(Some(Ok(candidate)));
        }

        // Check if the pre-vote campaign has timed out
        if self.has_pre_vote_campaign_timeout() {
            println!("Warning: Pre-vote campaign timeout occurred, stepping down as a follower");
            return Ok(Some(Err(Follower::new(None))));
        }
        Ok(None)
    }

    fn update_pre_vote_board(&self, response: PreVoteResponse) {
        self.pre_vote_board
            .insert(response.member_id, Some(response));
    }

    // Checks if the pre-candidate has granted a quorum of pre-votes from other members
    fn has_pre_vote_quorum(&self) -> bool {
        let num_matched = self
            .pre_vote_board
            .iter()
            .filter(|entry| matches!(entry.value(), Some(r) if r.vote_granted))
            .count();

        // + 1 accounts for the pre-candidate's implicit self-vote, which is not in the board
        (num_matched + 1) >= (self.pre_vote_board.len() + 1) / 2 + 1
    }

    // Checks if the pre-vote campaign has timed out to avoid a potential infinite hang
    fn has_pre_vote_campaign_timeout(&self) -> bool {
        self.pre_vote_campaign_deadline.is_some()
            && time::Instant::now() > self.pre_vote_campaign_deadline.unwrap()
    }
}

#[async_trait]
impl PaxosRole for PreCandidate {
    async fn handle_event(
        self,
        event: PaxosEvent,
        ctx: Arc<PaxosSharedContext>,
    ) -> Result<PaxosState, Box<dyn Error + Send + Sync>> {
        match event {
            PaxosEvent::LeaderLeaseExpired => {
                // The pre-candidate is already in a leader election process, ignore leader lease expiration event
                Ok(PaxosState::PreCandidate(self))
            }
            PaxosEvent::PreVoteRequestReceived(pre_vote_command) => {
                let request = pre_vote_command.get_request();
                let response = util::handle_pre_vote_request(request, ctx.clone());
                pre_vote_command.send(response)?;
                Ok(PaxosState::PreCandidate(self))
            }
            PaxosEvent::PreVoteResponseReceived(response) => {
                if let Some(result) = self.handle_pre_vote_response(response, ctx.clone()).await? {
                    return match result {
                        Ok(candidate) => Ok(PaxosState::Candidate(candidate)),
                        Err(follower) => Ok(PaxosState::Follower(follower)),
                    };
                }
                Ok(PaxosState::PreCandidate(self))
            }
            PaxosEvent::VoteRequestReceived(_) => {
                todo!()
            }
            PaxosEvent::VoteResponseReceived(_) => {
                todo!()
            }
        }
    }
}
