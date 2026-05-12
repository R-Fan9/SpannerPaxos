use crate::{PaxosEvent, PreVoteResponse, PaxosSharedContext};
use crate::roles::{Candidate, Follower, PaxosRole, util};
use crate::state_machine::PaxosState;
use dashmap::DashMap;
use std::error::Error;
use std::sync::Arc;
use tokio::time;
use tonic::async_trait;
use uuid::Uuid;

// The state of Paxos group leader pre-candidate - Thread-safe without external locking
pub struct PreCandidate {
    // A concurrent map of Paxos group member IDs to their local WAL positions
    pre_vote_board: DashMap<Uuid, Option<bool>>,

    // The deadline for the pre-vote campaign
    pre_vote_campaign_deadline: Option<time::Instant>,
}

impl PreCandidate {
    pub fn new() -> Self {
        Self {
            pre_vote_board: DashMap::new(),
            pre_vote_campaign_deadline: None,
        }
    }
    pub fn update_pre_vote(&self, member_id: Uuid, granted: bool) {
        self.pre_vote_board.insert(member_id, Some(granted));
    }

    // Checks if the pre-candidate has been rejected by a quorum of members for pre-vote
    pub fn has_pre_vote_rejected(&self) -> bool {
        let num_matched = self
            .pre_vote_board
            .iter()
            .filter(|entry| *entry.value() == Some(false))
            .count();

        // + 1 account for the pre-candidate, which is not in the board
        num_matched >= (self.pre_vote_board.len() + 1) / 2 + 1
    }

    // Checks if the pre-candidate has granted a quorum of pre-votes from other members
    pub fn has_pre_vote_quorum(&self) -> bool {
        let num_matched = self
            .pre_vote_board
            .iter()
            .filter(|entry| *entry.value() == Some(true))
            .count();

        // + 1 accounts for the pre-candidate's implicit self-vote, which is not in the board
        (num_matched + 1) >= (self.pre_vote_board.len() + 1) / 2 + 1
    }

    // Checks if the pre-vote campaign has timed out to avoid a potential infinite hang
    fn has_pre_vote_campaign_timeout(&self) -> bool {
        self.pre_vote_campaign_deadline.is_some()
            && time::Instant::now() > self.pre_vote_campaign_deadline.unwrap()
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
        let member_id = response.member_id;
        let granted = response.vote_granted;

        // Keep track of the pre-vote response from the remote member
        self.update_pre_vote(member_id, granted);

        // Check if a quorum of members has granted the pre-vote
        if self.has_pre_vote_quorum() {
            // Transition to a leader candidate
            let candidate = Candidate::new();

            // Increment the current term number
            ctx.increment_current_term();

            // Dispatch vote requests to other members to grant votes to become a leader
            candidate.dispatch_vote(ctx).await?;
            return Ok(Some(Ok(candidate)));
        }

        // Check if a quorum of members has rejected the pre-vote or the pre-vote campaign has timed out
        if self.has_pre_vote_rejected() || self.has_pre_vote_campaign_timeout() {
            // Step down as a follower
            let follower = Follower::new();
            return Ok(Some(Err(follower)));
        }
        Ok(None)
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
                        Ok(candidate) => {
                            println!(
                                "Info: A quorum of pre-votes has been granted, transitioned to leader candidate"
                            );
                            Ok(PaxosState::Candidate(candidate))
                        }
                        Err(follower) => {
                            println!(
                                "Warning: A quorum of pre-votes has been rejected or timeout occurred, step down as a follower"
                            );
                            Ok(PaxosState::Follower(follower))
                        }
                    };
                }
                Ok(PaxosState::PreCandidate(self))
            }
        }
    }
}
