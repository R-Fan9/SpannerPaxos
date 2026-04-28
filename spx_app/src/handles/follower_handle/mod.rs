use crate::commands::ReplicateWriteCommand;
use crate::configs::FollowerConfig;
use crate::handles::types::ReplicateWriteResult;
use spx_core::states::LeaderState;
use spx_lib::task_dispatcher::TaskDispatcher;
use spx_lib::worker_runner::{Worker, WorkerRunner};
use std::error::Error;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub mod types;

mod util;

// A handle for dispatching requests to a follower and processes the follower's responses in the background
pub struct FollowerHandle {
    // The runner for dispatching replicate write requests to the follower
    write_runner: WorkerRunner<
        TaskDispatcher<ReplicateWriteCommand, (ReplicateWriteCommand, ReplicateWriteResult)>,
    >,
}

impl FollowerHandle {
    pub async fn start(
        follower_config: FollowerConfig,
        leader_state: Arc<LeaderState>,
        cancellation_token: CancellationToken,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let client = util::create_follower_client(follower_config).await?;
        let write_runner = util::create_replicate_write_dispatcher(leader_state, client)
            .run(cancellation_token)
            .await?;

        Ok(Self { write_runner })
    }

    pub async fn stop(
        &mut self,
        cancellation_token: CancellationToken,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.write_runner.stop(cancellation_token).await?;
        Ok(())
    }

    // Dispatches a replicate write request to the follower
    pub fn dispatch_write(&self, command: ReplicateWriteCommand) {
        self.write_runner.get_worker().dispatch(command);
    }
}
