use crate::{PaxosCommand, PreVoteRequest, PreVoteResponse};
use std::fmt;

pub enum PaxosEvent {
    LeaderLeaseExpired,

    // This member has received a pre-vote message from a leader pre-candidate
    PreVoteRequestReceived(PaxosCommand<PreVoteRequest, PreVoteResponse>),

    // This leader pre-candidate has received a pre-vote response from another member
    PreVoteResponseReceived(PreVoteResponse),
}

impl PaxosEvent {
    pub fn get_term(&self) -> u32 {
        todo!()
    }
}

impl fmt::Display for PaxosEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            _ => {
                todo!()
            }
        }
    }
}
