use crate::state_machine::PaxosState;
use crate::{PaxosEvent, PaxosSharedContext};
use std::error::Error;
use std::sync::Arc;
use tonic::async_trait;

mod candidate;
mod follower;
mod leader;
mod precandidate;
mod util;

pub use candidate::Candidate;
pub use follower::Follower;
pub use leader::Leader;
pub use precandidate::PreCandidate;

/// A trait defining the methods that a Paxos group member must implement
#[async_trait]
pub trait PaxosRole {
    // Handles an event, returning a new state if the event triggers a transition or None to stay
    async fn handle_event(
        self,
        event: PaxosEvent,
        ctx: Arc<PaxosSharedContext>,
    ) -> Result<PaxosState, Box<dyn Error + Send + Sync>>;
}
