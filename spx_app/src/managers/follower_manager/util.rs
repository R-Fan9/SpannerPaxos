use crate::commands::ReplicateWriteCommand;
use crate::handles::{ReplicateWriteHandler, ReplicateWriteResult};
use spx_core::processors::ReplicateWriteProcessor;
use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use uuid::Uuid;

/// Creates a handler for processing replicate write responses from followers
pub fn create_replicate_write_handler(processor: ReplicateWriteProcessor) -> ReplicateWriteHandler {
    Arc::new(
        move |(command, result): (ReplicateWriteCommand, ReplicateWriteResult)| {
            let processor = processor.clone();
            Box::pin(async move {
                if let Err(e) = handle_replicate_write_response(processor, command, result).await {
                    eprintln!("Error handling replicate write response: {}", e);
                }
            }) as Pin<Box<dyn Future<Output = ()> + Send + 'static>>
        },
    )
}

/// Processes a replicate write response from a follower
async fn handle_replicate_write_response(
    processor: ReplicateWriteProcessor,
    command: ReplicateWriteCommand,
    result: ReplicateWriteResult,
) -> Result<(), Box<dyn Error>> {
    let resp = result?.into_inner();
    let follower_id = Uuid::parse_str(&resp.follower_id)?;

    if processor.quorum_reached(follower_id, resp.term_number, resp.slot_number)? {
        command.send(Ok(())).await?;
    }
    Ok(())
}
