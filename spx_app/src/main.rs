use crate::configs::ServerConfig;
use crate::grpc::GrpcPaxosDispatcher;
use spx_core::{PaxosEvent, PaxosStateMachine};
use spx_lib::worker_runner::{Worker, WorkerRunnerManager};
use std::collections::HashSet;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

mod configs;
mod grpc;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    // Create a cancellation token for signaling server shutdown
    let shutdown_token = CancellationToken::new();

    // Load configurations into memory
    let member_id = uuid::Uuid::new_v4();
    let member_configs = HashSet::new();
    let server_config = ServerConfig {
        host_address: "".to_string(),
        port: 0,
    };

    // Create a channel for sending Paxos events to be processed
    let (event_tx, event_rx) = mpsc::channel::<PaxosEvent>(1024);

    // Start the Paxos gRPC server
    grpc::start_paxos_server(server_config, event_tx.clone(), shutdown_token.clone())
        .await
        .expect("Failed to start Paxos gRPC server");

    // Create a dispatcher for dispatching Paxos requests with gRPC
    let dispatcher = GrpcPaxosDispatcher::new(event_tx.clone(), member_configs);

    // Create the Paxos state machine
    let machine = PaxosStateMachine::new(member_id, Arc::new(dispatcher), event_rx)
        .run(shutdown_token.child_token())
        .await
        .expect("Failed to start Paxos state machine");

    // Create a manager for managing the lifecycle of Paxos state machine and handles shutdown signals
    let mut runner_manager = WorkerRunnerManager::start_with(vec![machine], shutdown_token.clone())
        .await
        .expect("Failed to start worker runner manager");

    // Await for the shutdown token to be canceled
    shutdown_token.cancelled().await;

    // Perform a graceful shutdown for all running workers managed by the workers manager
    runner_manager
        .stop(CancellationToken::new())
        .await
        .expect("Error occurred during the graceful shutdown of background workers");

    Ok(())
}
