use std::error::Error;
use tokio::sync::oneshot::Sender;

// A command used to deliver and signal the completion of a Paxos operation
pub struct PaxosCommand<T, O = ()>
where
    T: Clone + Send + Sync,
    O: Send + Sync + 'static,
{
    // The request of the Paxos operation
    request: T,

    // A sender to send the result of the Paxos operation
    response_tx: Sender<O>,
}

impl<T, O> PaxosCommand<T, O>
where
    T: Clone + Send + Sync,
    O: Send + Sync + 'static,
{
    pub fn new(request: T, quorum_notify: Sender<O>) -> Self {
        Self {
            request,
            response_tx: quorum_notify,
        }
    }

    // Send the result of a Paxos operation
    pub fn send(self, result: O) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.response_tx
            .send(result)
            .map_err(|_| "The command receiver has been dropped")?;
        Ok(())
    }

    // Returns the Paxos operation request
    pub fn get_request(&self) -> T {
        self.request.clone()
    }
}
