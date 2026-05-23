use crate::grpc::util;
use spx_core::{PaxosCommand, PaxosEvent};
use spx_protocol::paxos_server::Paxos;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tonic::{Request, Response, Status, async_trait};

pub struct GrpcPaxosService {
    event_tx: Sender<PaxosEvent>,
}

impl GrpcPaxosService {
    pub fn new(event_tx: Sender<PaxosEvent>) -> Self {
        Self { event_tx }
    }

    async fn dispatch<Req, Resp>(
        &self,
        request: Req,
        make_event: impl FnOnce(PaxosCommand<Req, Resp>) -> PaxosEvent,
    ) -> Result<Resp, Status>
    where
        Req: Clone + Send + Sync + 'static,
        Resp: Send + Sync + 'static,
    {
        let (resp_tx, resp_rx) = oneshot::channel::<Resp>();
        let command = PaxosCommand::new(request, resp_tx);

        if let Err(e) = self.event_tx.send(make_event(command)).await {
            return Err(Status::internal(format!(
                "Failed to send Paxos event: {}",
                e
            )));
        }

        resp_rx
            .await
            .map_err(|e| Status::internal(format!("Failed to receive Paxos response: {}", e)))
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
        let request = util::pre_vote_request_from_proto(request.into_inner());
        let response = self
            .dispatch(request, PaxosEvent::PreVoteRequestReceived)
            .await?;
        Ok(Response::new(util::pre_vote_response_to_proto(response)))
    }

    async fn request_vote(
        &self,
        request: Request<spx_protocol::RequestVoteRequest>,
    ) -> Result<Response<spx_protocol::RequestVoteResponse>, Status> {
        let request = util::vote_request_from_proto(request.into_inner());
        let response = self
            .dispatch(request, PaxosEvent::VoteRequestReceived)
            .await?;
        Ok(Response::new(util::vote_response_to_proto(response)))
    }
}
