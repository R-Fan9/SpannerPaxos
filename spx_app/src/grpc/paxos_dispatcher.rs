use crate::configs::MemberConfig;
use crate::grpc::util;
use spx_core::{PaxosDispatcher, PaxosEvent, PreVoteRequest, VoteRequest};
use spx_protocol::paxos_client::PaxosClient;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::pin::Pin;
use tokio::sync::OnceCell;
use tokio::sync::mpsc::Sender;
use tonic::async_trait;
use tonic::transport::Channel;
use uuid::Uuid;

// A dispatcher for dispatching requests to the Paxos group members using gRPC
pub struct GrpcPaxosDispatcher {
    // The channel for sending the responses of Paxos requests as Paxos events
    event_tx: Sender<PaxosEvent>,

    // A map of Paxos member configurations to gRPC clients for communicating with the Paxos members
    paxos_clients: HashMap<Uuid, (MemberConfig, OnceCell<PaxosClient<Channel>>)>,
}

impl GrpcPaxosDispatcher {
    pub fn new(event_tx: Sender<PaxosEvent>, member_configs: HashSet<MemberConfig>) -> Self {
        let mut paxos_clients = HashMap::new();
        for config in member_configs {
            paxos_clients.insert(config.member_id, (config, OnceCell::new()));
        }
        Self {
            event_tx,
            paxos_clients,
        }
    }

    async fn get_paxos_client(
        &self,
        member_id: Uuid,
    ) -> Result<PaxosClient<Channel>, Box<dyn Error + Send + Sync>> {
        let (config, client) = self.paxos_clients.get(&member_id).unwrap();
        let client = client
            .get_or_try_init(|| util::create_paxos_client(config.clone()))
            .await?
            .clone();

        Ok(client)
    }

    async fn dispatch_request<F>(&self, action: F) -> Result<(), Box<dyn Error + Send + Sync>>
    where
        F: FnMut(
                PaxosClient<Channel>,
            ) -> Pin<
                Box<
                    dyn Future<Output = Result<PaxosEvent, Box<dyn Error + Send + Sync>>>
                        + Send
                        + 'static,
                >,
            > + Send
            + Sync
            + Clone
            + 'static,
    {
        for (id, _) in self.paxos_clients.iter() {
            let id = id.clone();
            let event_tx = self.event_tx.clone();
            let client = self.get_paxos_client(id.clone()).await?;
            let mut action = action.clone();

            tokio::spawn(async move {
                let event = match action(client).await {
                    Ok(event) => event,
                    Err(e) => {
                        eprintln!(
                            "Error: Failed to dispatch Paxos request to member {} with error {:?}",
                            id, e
                        );
                        return;
                    }
                };
                if event_tx.send(event).await.is_err() {
                    eprintln!("Error: Failed to send Paxos event to the event channel");
                };
            });
        }
        Ok(())
    }
}

#[async_trait]
impl PaxosDispatcher for GrpcPaxosDispatcher {
    async fn dispatch_prevote_request(
        &self,
        request: PreVoteRequest,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.dispatch_request(move |mut client| {
            let request = request.clone();
            Box::pin(async move {
                let request = util::pre_vote_request_to_proto(request);
                let response = match client.pre_vote(request).await {
                    Ok(response) => response,
                    Err(e) => return Err(format!("Pre-vote request failed {:?}", e).into()),
                };
                Ok(PaxosEvent::PreVoteResponseReceived(
                    util::pre_vote_response_from_proto(response.into_inner()),
                ))
            })
        })
        .await?;
        Ok(())
    }

    async fn dispatch_vote_request(
        &self,
        request: VoteRequest,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.dispatch_request(move |mut client| {
            let request = request.clone();
            Box::pin(async move {
                let request = util::vote_request_to_proto(request);
                let response = match client.request_vote(request).await {
                    Ok(response) => response,
                    Err(e) => return Err(format!("Vote request failed {:?}", e).into()),
                };
                Ok(PaxosEvent::VoteResponseReceived(
                    util::vote_response_from_proto(response.into_inner()),
                ))
            })
        })
        .await?;
        Ok(())
    }
}
