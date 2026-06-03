use crate::worker_runner::{Worker, WorkerRunner};
use async_trait::async_trait;
use std::error::Error;
use std::sync::Arc;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

// A manager that enables graceful shutdown of its managed runners when application shutdown is signaled
pub struct WorkerRunnerManager<T>
where
    T: Worker,
{
    runners: Mutex<Vec<WorkerRunner<T>>>,
}

impl<T> WorkerRunnerManager<T>
where
    T: Worker + 'static,
{
    pub async fn start_with(
        runners: Vec<WorkerRunner<T>>,
        cancellation_token: CancellationToken,
    ) -> Result<WorkerRunner<WorkerRunnerManager<T>>, Box<dyn Error + Send + Sync>> {
        let runner_manager = WorkerRunnerManager::new();
        for worker in runners {
            runner_manager.add_runner(worker).await;
        }
        let runner_manger = runner_manager.run(cancellation_token).await?;
        Ok(runner_manger)
    }

    pub fn new() -> Self {
        Self {
            runners: Mutex::new(Vec::new()),
        }
    }

    pub async fn add_runner(&self, runner: WorkerRunner<T>) {
        self.runners.lock().await.push(runner);
    }
}

#[async_trait]
impl<T> Worker for WorkerRunnerManager<T>
where
    T: Worker,
{
    async fn on_start(
        self: Arc<Self>,
        cancellation_token: CancellationToken,
    ) -> Result<JoinHandle<Result<(), Box<dyn Error + Send + Sync>>>, Box<dyn Error + Send + Sync>>
    {
        let sigint_token = cancellation_token.clone();
        let sigterm_token = cancellation_token.clone();

        // Create a Signal object to listen for SIGTERM signal
        let mut sigterm_signal =
            signal(SignalKind::terminate()).expect("Failed to listen for SIGTERM");

        let task = tokio::spawn(async move {
            tokio::select! {
                _ = cancellation_token.cancelled() => {
                    println!("Application shutdown has been signal. Initiating graceful shutdown.");
                },

                _ = tokio::signal::ctrl_c() => {
                    println!("Received SIGINT (Ctrl+C). Initiating graceful shutdown.");
                    sigint_token.cancel();
                },

                _ = sigterm_signal.recv() => {
                    println!("Received SIGTERM. Initiating graceful shutdown.");
                    sigterm_token.cancel();
                }
            }
            Ok(())
        });
        Ok(task)
    }

    async fn on_stop(
        &self,
        cancellation_token: CancellationToken,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Invoke graceful shutdown for all existing runners
        for runner in self.runners.lock().await.iter_mut() {
            if let Err(e) = runner.stop(cancellation_token.clone()).await {
                eprint!("Error: error stopping worker: {:?}", e)
            }
        }
        Ok(())
    }
}
