use crate::commands::ReplicateWriteCommand;
use crate::handles::{FollowerHandle, ReplicateWriteHandler};
use crate::payloads::ReplicateWritePayload;
use spx_client::FollowerServiceClient;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tonic::transport::Channel;
use uuid::Uuid;

// A manager that handles follower-related operations
pub struct FollowerManager {
    follower_ids: HashSet<Uuid>,
    followers_handles: Option<HashMap<Uuid, FollowerHandle>>,
}

impl FollowerManager {
    pub fn new(follower_ids: HashSet<Uuid>) -> Self {
        Self {
            follower_ids,
            followers_handles: None,
        }
    }

    pub async fn start_handles(
        &mut self,
        replicate_write_handler: ReplicateWriteHandler,
        cancellation_token: CancellationToken,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut handles = HashMap::new();
        for id in self.follower_ids.iter() {
            // Create a gRPC client for the follower
            let follower_client = Self::create_follower_client(id.clone()).await?;

            // Start a handle for follower-related operations
            let handle = FollowerHandle::start(
                follower_client,
                replicate_write_handler.clone(),
                cancellation_token.clone(),
            )
            .await?;

            handles.insert(id.clone(), handle);
        }

        self.followers_handles = Some(handles);
        Ok(())
    }

    pub async fn stop_handles(
        &mut self,
        cancellation_token: CancellationToken,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let handles = self
            .followers_handles
            .as_mut()
            .ok_or("Followers handles not started")?;

        // Stop all follower handles
        for (_, handle) in handles.iter_mut() {
            handle.stop(cancellation_token.clone()).await?;
        }
        Ok(())
    }

    pub async fn replicate_write<F>(
        &self,
        payload: ReplicateWritePayload,
        mut dispatch_callback: F,
    ) -> Result<(), Box<dyn Error>>
    where
        F: FnMut(Uuid),
    {
        let handles = self
            .followers_handles
            .as_ref()
            .ok_or("Followers handles not started")?;

        // Create a channel for signaling a quorum of followers has completed the replicate write request
        let (quorum_tx, mut quorum_rx) = mpsc::channel(1);

        // Dispatch the replicate write request to all followers
        for (id, handle) in handles.iter() {
            let command = ReplicateWriteCommand::new(payload.clone(), quorum_tx.clone());
            handle.dispatch_write(command);
            dispatch_callback(id.clone());
        }

        // Drop the original sender reference to ensure the channel can be closed
        drop(quorum_tx);

        // Wait for a quorum of followers to replicate the log entry
        quorum_rx.recv().await;
        Ok(())
    }

    async fn create_follower_client(
        id: Uuid,
    ) -> Result<FollowerServiceClient<Channel>, Box<dyn Error + Send + Sync>> {
        // TODO - implement proper follower gRPC client connection
        let endpoint = format!("http://localhost:{}", id);
        Ok(FollowerServiceClient::connect(endpoint).await?)
    }
}
