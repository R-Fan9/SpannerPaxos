use crate::task_dispatcher::{Executor, Handler};
use crate::worker_runner::Worker;
use async_trait::async_trait;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// A dispatcher that continuously dispatches the execution of tasks upon request
/// and process the results of the execution in the background
///
/// # Type Parameters
///
/// * `I` - The input type of the execution
/// * `O` - The output type of the execution
pub struct TaskDispatcher<I, O>
where
    I: Send + Sync + 'static,
    O: Send + Sync + 'static,
{
    // A channel for sending the output of the execution
    output_tx: mpsc::Sender<O>,

    // A channel for receiving the output of the execution
    output_rx: Mutex<mpsc::Receiver<O>>,

    // The executor for executing the tasks
    executor: Executor<I, O>,

    // The handler for handling the output of tasks
    handler: Handler<O>,
}

impl<I, O> TaskDispatcher<I, O>
where
    I: Send + Sync + 'static,
    O: Send + Sync + 'static,
{
    // Create and start a new dispatcher instance with the specified executor and result handler
    pub fn new(executor: Executor<I, O>, handler: Handler<O>) -> Self {
        let (output_tx, output_rx) = mpsc::channel(1024);
        Self {
            output_tx,
            output_rx: Mutex::new(output_rx),
            executor,
            handler,
        }
    }

    // Dispatch a task to be executed
    pub fn dispatch(&self, input: I) {
        let output_tx = self.output_tx.clone();
        let executor = self.executor.clone();

        // Spawn a new task to follower_handle the task execution
        tokio::spawn(async move {
            let output = executor(input).await;

            // Send the task execution result to the channel for further processing
            if output_tx.send(output).await.is_err() {
                eprintln!("Send task result to channel failed.");
            };
        });
    }

    // Process the results of the execution in the background
    async fn process_outputs(
        self: Arc<Self>,
        cancellation_token: CancellationToken,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut output_rx = self.output_rx.lock().await;
        loop {
            tokio::select! {
                // This ensures the cancellation branch is checked first if multiple branches are ready
                biased;

                // If the cancellation token is triggered, break out of the loop
                _ = cancellation_token.cancelled() => {
                    break;
                }

                // Wait for the next execution result to be processed
                maybe_res = output_rx.recv()=> {
                    match maybe_res {
                        Some(res) => {
                            (self.handler)(res).await;
                        }
                        None => {
                            // Channel closed
                            eprintln!("No task result received from channel.");
                            break;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl<I, O> Worker for TaskDispatcher<I, O>
where
    I: Send + Sync + 'static,
    O: Send + Sync + 'static,
{
    async fn on_start(
        self: Arc<Self>,
        cancellation_token: CancellationToken,
    ) -> Result<JoinHandle<Result<(), Box<dyn Error + Send + Sync>>>, Box<dyn Error + Send + Sync>>
    {
        let task = tokio::spawn(async move { self.process_outputs(cancellation_token).await });
        Ok(task)
    }

    async fn on_stop(
        &self,
        cancellation_token: CancellationToken,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        Ok(())
    }
}
