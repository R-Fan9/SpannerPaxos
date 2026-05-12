use uuid::Uuid;
use crate::configs::ServerConfig;

#[derive(Clone, Debug)]
pub struct MemberConfig {
    pub member_id: Uuid,
    pub server: ServerConfig,
}
