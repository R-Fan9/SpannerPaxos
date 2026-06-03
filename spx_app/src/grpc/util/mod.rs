use crate::configs::{MemberConfig, ServerConfig};
use spx_protocol::paxos_client::PaxosClient;
use std::error::Error;
use tokio::net::TcpListener;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use tonic::codegen::tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::{Channel, Server};

mod proto_conversions;

use crate::grpc::GrpcPaxosService;
pub use proto_conversions::*;
use spx_core::PaxosEvent;
use spx_protocol::paxos_server::PaxosServer;

pub(super) async fn create_paxos_client(
    config: MemberConfig,
) -> Result<PaxosClient<Channel>, Box<dyn Error + Send + Sync>> {
    // TODO - implement proper follower gRPC client connection
    let endpoint = format!(
        "http://{}:{}",
        config.server.host_address, config.server.port
    );
    Ok(PaxosClient::connect(endpoint).await?)
}

pub async fn start_paxos_server(
    config: ServerConfig,
    event_tx: Sender<PaxosEvent>,
    cancellation_token: CancellationToken,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    // Create a TCP listener stream for incoming gRPC requests
    let listener = TcpListener::bind((config.host_address, config.port)).await?;
    let incoming = TcpListenerStream::new(listener);

    // Create the Paxos service for handling gRPC requests
    let paxos_service = GrpcPaxosService::new(event_tx);

    // Start the Paxos gRPC server
    tokio::spawn(async move {
        if let Err(err) = Server::builder()
            .add_service(PaxosServer::new(paxos_service))
            .serve_with_incoming_shutdown(incoming, cancellation_token.clone().cancelled())
            .await
        {
            eprintln!("Paxos gRPC server error: {}", err);

            // Signal Paxos server shutdown when its gRPC server exited with an error
            cancellation_token.cancel()
        }
    });
    Ok(())
}
