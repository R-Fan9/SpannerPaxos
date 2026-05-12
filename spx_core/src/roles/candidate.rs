use crate::roles::{PaxosRole, util};
use crate::state_machine::PaxosState;
use crate::{PaxosEvent, PaxosSharedContext};
use std::error::Error;
use std::sync::Arc;
use tonic::async_trait;

pub struct Candidate {}

impl Candidate {
    pub fn new() -> Self {
        Self {}
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
            PaxosEvent::PreVoteResponseReceived(_) => {
                eprint!("Error: Received an unexpected pre-vote response as a candidate");
                Ok(PaxosState::Candidate(self))
            }
        }
    }
}
