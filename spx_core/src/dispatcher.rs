use crate::{PreVoteRequest, VoteRequest};
use std::error::Error;
use tonic::async_trait;

/// A trait defining the methods to be implemented to dispatch Paxos requests to other members
#[async_trait]
pub trait PaxosDispatcher: Send + Sync {
    // Dispatch pre-vote requests to all other members
    async fn dispatch_prevote_request(
        &self,
        request: PreVoteRequest,
    ) -> Result<(), Box<dyn Error + Send + Sync>>;

    // Dispatch vote requests to all other members
    async fn dispatch_vote_request(
        &self,
        request: VoteRequest,
    ) -> Result<(), Box<dyn Error + Send + Sync>>;
}
