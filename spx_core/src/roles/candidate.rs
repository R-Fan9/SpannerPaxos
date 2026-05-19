use crate::roles::{Follower, PaxosRole, util};
use crate::state_machine::PaxosState;
use crate::{PaxosEvent, PaxosSharedContext, PreVoteResponse};
use std::error::Error;
use std::sync::Arc;
use tonic::async_trait;

pub struct Candidate {}

impl Candidate {
    pub fn new() -> Self {
        Self {}
    }

    fn handle_pre_vote_response(
        response: PreVoteResponse,
        ctx: &PaxosSharedContext,
    ) -> Option<Follower> {
        let leader_id = util::get_active_leader(&response, ctx.get_current_term(), |received_term, local_term| received_term > local_term)?;
        println!(
            "Info: Member {} reported active leader {}, stepping down as a follower",
            response.member_id, leader_id
        );
        Some(Follower::new(Some(leader_id)))
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
}

#[async_trait]
impl PaxosRole for Candidate {
    async fn handle_event(
        self,
        event: PaxosEvent,
        ctx: Arc<PaxosSharedContext>,
    ) -> Result<PaxosState, Box<dyn Error + Send + Sync>> {
        match event {
            PaxosEvent::LeaderLeaseExpired => {
                // The candidate is already in a leader election process, ignore leader lease expiration event
                Ok(PaxosState::Candidate(self))
            }
            PaxosEvent::PreVoteRequestReceived(pre_vote_command) => {
                let request = pre_vote_command.get_request();
                let response = util::handle_pre_vote_request(request, ctx.clone());
                pre_vote_command.send(response)?;
                Ok(PaxosState::Candidate(self))
            }
            PaxosEvent::PreVoteResponseReceived(response) => {
                if let Some(follower) = Self::handle_pre_vote_response(response, &ctx) {
                    return Ok(PaxosState::Follower(follower));
                }
                Ok(PaxosState::Candidate(self))
            }
        }
    }
}
