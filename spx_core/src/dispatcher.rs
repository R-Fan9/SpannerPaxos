use crate::{PreVoteRequest, ReplicateWriteRequest, VoteRequest};
use std::error::Error;
use std::sync::Arc;
use tonic::async_trait;
use uuid::Uuid;


/// A trait defining the methods to be implemented to dispatch Paxos requests to other members
#[async_trait]
pub trait PaxosDispatcher: Send + Sync {
    // Dispatch pre-vote requests to all other members
    async fn dispatch_prevote_request(
        &self,
        request: PreVoteRequest,
    ) -> Result<(), Box<dyn Error + Send + Sync>>;

    // Dispatch vote requests to all other members; on_dispatch is invoked per member
    // with the member's ID immediately after the request is sent to that member
    async fn dispatch_vote_request(
        &self,
        request: VoteRequest,
        on_dispatch: Arc<dyn Fn(Uuid) + Send + Sync>,
    ) -> Result<(), Box<dyn Error + Send + Sync>>;

    // Dispatch replicate write requests to all other members; on_dispatch is invoked per member
    // with the member's ID immediately after the request is sent to that member
    async fn dispatch_replicate_write_request(
        &self,
        request: ReplicateWriteRequest,
        on_dispatch: Arc<dyn Fn(Uuid) + Send + Sync>,
    ) -> Result<(), Box<dyn Error + Send + Sync>>;
}
