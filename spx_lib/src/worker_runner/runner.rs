use super::RunStatus;
use super::Worker;
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// A runner that manages the lifecycle (start/stop) of a background worker task
///
/// # Type Parameters
///
/// * `T` - The type of the worker
pub struct WorkerRunner<T>
where
    T: Worker,
{
    worker: Arc<T>,
    worker_task: Option<JoinHandle<Result<(), Box<dyn Error + Send + Sync>>>>,
    run_status: AtomicU8,
    stop_token: Option<CancellationToken>,
}

impl<T> WorkerRunner<T>
where
    T: Worker,
{
    pub fn new(worker: T) -> Self {
        Self {
            worker: Arc::new(worker),
            worker_task: None,
            run_status: AtomicU8::new(RunStatus::IDLE.into()),
            stop_token: None,
        }
    }

    pub fn get_worker(&self) -> Arc<T> {
        self.worker.clone()
    }

    pub async fn start(
        &mut self,
        cancellation_token: CancellationToken,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Try to transition to the ACTIVE state. If already running, skip.
        if !self.try_switch_run_status(true) {
            return Ok(());
        }

        // Create a child token that can be canceled internally
        let child_token = cancellation_token.child_token();
        self.stop_token = Some(child_token.clone());

        // Start running the worker task in the background
        let worker_task = self.worker.clone().on_start(child_token).await?;
        self.worker_task = Some(worker_task);

        Ok(())
    }

    pub async fn stop(
        &mut self,
        cancellation_token: CancellationToken,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Check if the stop token exists, meaning the worker was started
        let Some(stop_token) = self.stop_token.take() else {
            // The worker task hasn't started yet, thus cannot be stopped
            return Ok(());
        };

        // Cancel the token to signal stop for the running worker task
        stop_token.cancel();

        // Execute the worker's custom stop logic
        self.worker.on_stop(cancellation_token.clone()).await?;

        // Await the completion of the background running worker task
        if let Some(task) = self.worker_task.take() {
            Self::stop_worker_task(task, stop_token).await?;
        }

        // Reset the runner back to its initial state
        self.reset();
        Ok(())
    }

    fn try_switch_run_status(&self, to_running: bool) -> bool {
        let (expected_current_state, expected_new_state) = if to_running {
            (RunStatus::IDLE, RunStatus::ACTIVE)
        } else {
            (RunStatus::ACTIVE, RunStatus::IDLE)
        };

        // Returns true only if the run status is updated
        self.run_status
            .compare_exchange(
                expected_current_state.into(),
                expected_new_state.into(),
                Ordering::Release,
                Ordering::Relaxed,
            )
            .is_ok()
    }

    fn reset(&mut self) {
        self.worker_task = None;
        self.stop_token = None;

        // Transition back to the IDLE state
        self.try_switch_run_status(false);
    }

    async fn stop_worker_task(
        mut task: JoinHandle<Result<(), Box<dyn Error + Send + Sync>>>,
        cancellation_token: CancellationToken,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        tokio::select! {
            // The worker task finishes gracefully
            result = &mut task => {
                result??;
                return Ok(());
            }
            // The external cancellation token is triggered first
            _ = cancellation_token.cancelled() => {
                // Abort the task so it doesn't continue running detached in the background
                task.abort();

                // Return an error to explicitly indicate that the shutdown was forced/cancelled
                return Err("Shutdown cancelled: worker task was forcefully aborted".into());
            }
        }
    }
}
