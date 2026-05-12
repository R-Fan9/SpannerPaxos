mod spx {
    pub mod messages {
        tonic::include_proto!("spx.messages");
    }
    pub mod services {
        tonic::include_proto!("spx.services");
    }
}

pub use spx::messages::*;
pub use spx::services::*;
