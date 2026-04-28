use crate::commands::ReplicateWriteCommand;
use crate::configs::FollowerConfig;
use crate::handles::FollowerHandle;
use crate::payloads::ReplicateWritePayload;
use spx_core::states::LeaderState;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

// A manager that handles follower-related operations
pub struct FollowerManager {
    follower_configs: HashSet<FollowerConfig>,
    follower_handles: Option<HashMap<Uuid, FollowerHandle>>,
}

impl FollowerManager {
    pub fn new(follower_configs: HashSet<FollowerConfig>) -> Self {
        Self {
            follower_configs,
            follower_handles: None,
        }
    }

    pub async fn start_handles(
        &mut self,
        leader_state: Arc<LeaderState>,
        cancellation_token: CancellationToken,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut handles = HashMap::new();
        for config in self.follower_configs.iter() {
            // Start a handle for dispatching and processing follower-related operations
            let handle = FollowerHandle::start(
                config.clone(),
                leader_state.clone(),
                cancellation_token.clone(),
            )
            .await?;
            handles.insert(config.follower_id, handle);
        }

        self.follower_handles = Some(handles);
        Ok(())
    }

    pub async fn stop_handles(
        &mut self,
        cancellation_token: CancellationToken,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let handles = self
            .follower_handles
            .as_mut()
            .ok_or("Followers handles not started")?;

        // Stop all follower handles
        for (_, handle) in handles.iter_mut() {
            handle.stop(cancellation_token.clone()).await?;
        }
        Ok(())
    }

    pub async fn replicate_write(
        &self,
        payload: ReplicateWritePayload,
        mut on_dispatched: impl FnMut(Uuid),
    ) -> Result<(), Box<dyn Error>> {
        let handles = self
            .follower_handles
            .as_ref()
            .ok_or("Followers handles not started")?;

        // Create a channel for signaling a quorum of followers has completed the replicate write request
        let (quorum_tx, mut quorum_rx) = mpsc::channel(1);

        // Dispatch the replicate write request to all followers
        for (id, handle) in handles.iter() {
            let command = ReplicateWriteCommand::new(payload.clone(), quorum_tx.clone());
            handle.dispatch_write(command);
            on_dispatched(id.clone());
        }

        // Drop the original sender reference to ensure the channel can be closed
        drop(quorum_tx);

        // Wait for a quorum of followers to replicate the log entry
        quorum_rx.recv().await;
        Ok(())
    }
}
