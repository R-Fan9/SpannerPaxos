use crate::commands::ReplicateWriteCommand;
use crate::handles::{ReplicateWriteHandler, ReplicateWriteResult};
use spx_client::FollowerServiceClient;
use spx_lib::task_dispatcher::TaskDispatcher;
use spx_lib::worker_runner::{Worker, WorkerRunner};
use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tonic::transport::Channel;

// A handle for dispatching requests to a follower and processes the follower's responses in the background
pub struct FollowerHandle {
    // The runner for dispatching replicate write requests to the follower
    write_runner: WorkerRunner<
        TaskDispatcher<ReplicateWriteCommand, (ReplicateWriteCommand, ReplicateWriteResult)>,
    >,
}

impl FollowerHandle {
    pub async fn start(
        client: FollowerServiceClient<Channel>,
        handler: ReplicateWriteHandler,
        cancellation_token: CancellationToken,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let write_runner = Self::create_write_dispatcher(client, handler)
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

    fn create_write_dispatcher(
        client: FollowerServiceClient<Channel>,
        handler: ReplicateWriteHandler,
    ) -> TaskDispatcher<ReplicateWriteCommand, (ReplicateWriteCommand, ReplicateWriteResult)> {
        let executor = Arc::new(move |command: ReplicateWriteCommand| {
            Self::replicate_write(client.clone(), command)
        });
        TaskDispatcher::new(executor, handler)
    }

    fn replicate_write(
        mut client: FollowerServiceClient<Channel>,
        command: ReplicateWriteCommand,
    ) -> Pin<Box<dyn Future<Output = (ReplicateWriteCommand, ReplicateWriteResult)> + Send>> {
        Box::pin(async move {
            let request = command.create_request();
            let resp = client.replicate_write(request).await;
            (command, resp)
        })
    }
}
