use crate::worker_runner::WorkerRunner;
use async_trait::async_trait;
use std::error::Error;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// A trait defining the lifecycle methods that a worker must implement
#[async_trait]
pub trait Worker: Send + Sync {
    // Start up the worker task running in the background
    async fn on_start(
        self: Arc<Self>,
        cancellation_token: CancellationToken,
    ) -> Result<JoinHandle<Result<(), Box<dyn Error + Send + Sync>>>, Box<dyn Error + Send + Sync>>;

    // Allow worker to implement their own custom on stop logic
    async fn on_stop(
        &self,
        cancellation_token: CancellationToken,
    ) -> Result<(), Box<dyn Error + Send + Sync>>;

    // Wrap the worker instance into a runner instance and start the background worker task
    async fn run(
        self: Self,
        cancellation_token: CancellationToken,
    ) -> Result<WorkerRunner<Self>, Box<dyn Error + Send + Sync>>
    where
        Self: Sized + 'static,
    {
        let mut runner = WorkerRunner::new(self);
        runner.start(cancellation_token).await?;
        Ok(runner)
    }
}
