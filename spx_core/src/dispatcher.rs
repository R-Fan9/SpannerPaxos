use crate::{AcceptRequest, ClientWriteRequest, ClientWriteResponse, PreVoteRequest, VoteRequest};
use std::error::Error;
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

    // Dispatch vote requests to all other members
    async fn dispatch_vote_request(
        &self,
        request: VoteRequest,
    ) -> Result<(), Box<dyn Error + Send + Sync>>;

    // Dispatch an accept (AppendEntries) request to a specific member
    async fn dispatch_accept_request(
        &self,
        member_id: Uuid,
        request: AcceptRequest,
    ) -> Result<(), Box<dyn Error + Send + Sync>>;

    // Forward a client write request to the current leader; spawns a background task that awaits
    // the leader's response and invokes on_response with it
    async fn dispatch_client_write(
        &self,
        leader_id: Uuid,
        request: ClientWriteRequest,
        on_response: Box<dyn FnOnce(ClientWriteResponse) + Send + 'static>,
    ) -> Result<(), Box<dyn Error + Send + Sync>>;
}
