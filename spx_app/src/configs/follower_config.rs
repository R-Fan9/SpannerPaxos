use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct FollowerConfig {
    pub follower_id: Uuid,
    pub host_address: String,
    pub port: u16,
}
