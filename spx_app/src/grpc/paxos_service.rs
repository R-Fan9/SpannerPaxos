use crate::grpc::util;
use spx_core::{PaxosCommand, PaxosEvent, PreVoteResponse};
use spx_protocol::paxos_server::Paxos;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tonic::{Request, Response, Status, async_trait};

pub struct GrpcPaxosService {
    // A channel for sending Paxos events to be processed by the Paxos state machine
    event_tx: Sender<PaxosEvent>,
}

impl GrpcPaxosService {
    pub fn new(event_tx: Sender<PaxosEvent>) -> Self {
        Self { event_tx }
    }
}

#[async_trait]
impl Paxos for GrpcPaxosService {
    async fn save_write(
        &self,
        request: Request<spx_protocol::SaveWriteRequest>,
    ) -> Result<Response<spx_protocol::SaveWriteResponse>, Status> {
        todo!()
    }

    async fn replicate_write(
        &self,
        request: Request<spx_protocol::ReplicateWriteRequest>,
    ) -> Result<Response<spx_protocol::ReplicateWriteResponse>, Status> {
        todo!()
    }

    async fn commit_write(
        &self,
        request: Request<spx_protocol::CommitWriteRequest>,
    ) -> Result<Response<spx_protocol::CommitWriteResponse>, Status> {
        todo!()
    }
    async fn pre_vote(
        &self,
        request: Request<spx_protocol::PreVoteRequest>,
    ) -> Result<Response<spx_protocol::PreVoteResponse>, Status> {
        let request = request.into_inner();
        let request = util::pre_vote_request_from_proto(request);
        let (resp_tx, resp_rx) = oneshot::channel::<PreVoteResponse>();
        let command = PaxosCommand::new(request, resp_tx);

        if let Err(e) = self
            .event_tx
            .send(PaxosEvent::PreVoteRequestReceived(command))
            .await
        {
            return Err(Status::internal(format!(
                "Failed to send pre-vote request: {}",
                e
            )));
        }

        match resp_rx.await {
            Ok(response) => {
                let response = util::pre_vote_response_to_proto(response);
                Ok(Response::new(response))
            }
            Err(e) => Err(Status::internal(format!(
                "Failed to receive pre-vote response: {}",
                e
            ))),
        }
    }

    async fn request_vote(
        &self,
        request: Request<spx_protocol::RequestVoteRequest>,
    ) -> Result<Response<spx_protocol::RequestVoteResponse>, Status> {
        todo!()
    }
}
