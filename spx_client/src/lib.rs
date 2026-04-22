pub use self::follower_service_client::*;
pub use self::leader_service_client::*;

tonic::include_proto!("spanner.paxos.services");
