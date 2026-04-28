use crate::commands::ReplicateWriteCommand;
use crate::handles::types::{ReplicateWriteExecutor, ReplicateWriteHandler, ReplicateWriteResult};
use spx_core::states::LeaderState;
use spx_lib::task_dispatcher::TaskDispatcher;
use spx_protocol::follower_client::FollowerClient;
use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tonic::transport::Channel;
use uuid::Uuid;

pub fn create_replicate_write_dispatcher(
    leader_state: Arc<LeaderState>,
    client: FollowerClient<Channel>,
) -> TaskDispatcher<ReplicateWriteCommand, (ReplicateWriteCommand, ReplicateWriteResult)> {
    let executor = create_replicate_write_executor(client.clone());
    let handler = create_replicate_write_handler(leader_state.clone());
    TaskDispatcher::new(executor, handler)
}

fn create_replicate_write_executor(client: FollowerClient<Channel>) -> ReplicateWriteExecutor {
    Arc::new(move |command: ReplicateWriteCommand| {
        let client = client.clone();
        Box::pin(async move { execute_replicate_write_request(client, command).await })
    })
}

async fn execute_replicate_write_request(
    mut client: FollowerClient<Channel>,
    command: ReplicateWriteCommand,
) -> (ReplicateWriteCommand, ReplicateWriteResult) {
    let request = command.create_request();
    let resp = client.replicate_write(request).await;
    (command, resp)
}

fn create_replicate_write_handler(leader_state: Arc<LeaderState>) -> ReplicateWriteHandler {
    Arc::new(
        move |(command, result): (ReplicateWriteCommand, ReplicateWriteResult)| {
            let leader_state = leader_state.clone();
            Box::pin(async move {
                if let Err(e) = handle_replicate_write_response(leader_state, command, result).await
                {
                    eprintln!("Error handling replicate write response: {}", e);
                }
            }) as Pin<Box<dyn Future<Output = ()> + Send + 'static>>
        },
    )
}

async fn handle_replicate_write_response(
    leader_state: Arc<LeaderState>,
    command: ReplicateWriteCommand,
    result: ReplicateWriteResult,
) -> Result<(), Box<dyn Error>> {
    let resp = result?.into_inner();
    if resp.term_number < leader_state.get_term_number() {
        // The response from the follower is for an older term
        return Err("Follower response is for an older term".into());
    }

    // Update the follower's match index to indicate that the log entry has been persisted by the follower
    let follower_id = Uuid::parse_str(&resp.follower_id)?;
    leader_state.update_match_index(resp.slot_number, Some(follower_id));

    if leader_state.has_committed(resp.slot_number) {
        // The log entry at the given slot number has already been committed, return true
        return Ok(());
    }

    if leader_state.has_quorum(resp.slot_number) {
        command.send(Ok(())).await?;
    }

    Ok(())
}
