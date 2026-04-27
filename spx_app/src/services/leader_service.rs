use crate::managers::FollowerManager;
use spx_core::operators::LeaderOperator;
use spx_protocol::leader_server::Leader;
use spx_protocol::{SaveWriteRequest, SaveWriteResponse};
use tonic::{Request, Response, Status};

// A gRPC service for handling leader-related operations
pub struct LeaderService {
    leader_operator: LeaderOperator,
    follower_manager: FollowerManager,
}

#[tonic::async_trait]
impl Leader for LeaderService {
    async fn save_write(
        &self,
        request: Request<SaveWriteRequest>,
    ) -> Result<Response<SaveWriteResponse>, Status> {
        let entry = request.into_inner().payload;
        let result = self
            .leader_operator
            .save_write(entry, |payload, on_dispatched| {
                self.follower_manager
                    .replicate_write(payload, on_dispatched)
            })
            .await;

        match result {
            Ok(_) => Ok(Response::new(SaveWriteResponse {})),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }
}
