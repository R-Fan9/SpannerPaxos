use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
pub struct ServerConfig {
    pub host_address: String,
    pub port: u16,
}
