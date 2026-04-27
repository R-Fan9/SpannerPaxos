use crate::configs::FollowerConfig;
use spx_protocol::follower_client::FollowerClient;
use std::error::Error;
use tonic::transport::Channel;

pub async fn create_follower_client(
    follower_config: FollowerConfig,
) -> Result<FollowerClient<Channel>, Box<dyn Error + Send + Sync>> {
    // TODO - implement proper follower gRPC client connection
    let endpoint = format!(
        "http://{}:{}",
        follower_config.host_address, follower_config.port
    );
    Ok(FollowerClient::connect(endpoint).await?)
}
