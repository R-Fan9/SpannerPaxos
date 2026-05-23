use crate::PaxosEvent;
use crate::context::PaxosSharedContext;
use crate::roles::{Candidate, Follower, Leader, PaxosRole, PreCandidate};
use std::error::Error;
use std::sync::Arc;
use uuid::Uuid;

// The state of a Paxos group member
pub enum PaxosState {
    Follower(Follower),
    PreCandidate(PreCandidate),
    Candidate(Candidate),
    Leader(Leader),
}

impl PaxosState {
    pub async fn process_event(
        self,
        event: PaxosEvent,
        ctx: Arc<PaxosSharedContext>,
    ) -> Result<PaxosState, Box<dyn Error + Send + Sync>> {
        let mut current_state = self;

        // Check if the event term is greater than the current term
        let event_term = event.get_term();
        if event_term > ctx.get_current_term() {
            // Update the current term on this member (node) to the highest term received
            ctx.set_current_term(event_term);

            // Force this member (node) to step down as a follower, preserving the known leader if any
            current_state = PaxosState::Follower(Follower::new(current_state.get_leader_id(), ctx.get_event_sender()));
        }

        let next_state = match current_state {
            PaxosState::Follower(follower) => follower.handle_event(event, ctx).await?,
            PaxosState::PreCandidate(pre_candidate) => {
                pre_candidate.handle_event(event, ctx).await?
            }
            PaxosState::Candidate(candidate) => candidate.handle_event(event, ctx).await?,
            PaxosState::Leader(leader) => leader.handle_event(event, ctx).await?,
        };

        Ok(next_state)
    }

    fn get_leader_id(&self) -> Option<Uuid> {
        match self {
            PaxosState::Follower(follower) => follower.get_current_leader_id(),
            _ => None,
        }
    }
}
