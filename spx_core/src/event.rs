use crate::{PaxosCommand, PreVoteRequest, PreVoteResponse, ReplicateWriteRequest, ReplicateWriteResponse, VoteRequest, VoteResponse};
use std::fmt;

pub enum PaxosEvent {
    // The leader lease has expired or it's not set
    LeaderLeaseExpired,

    // The election countdown has expired; if still a follower, transition to pre-candidate
    ElectionCountdownExpired,

    // The pre-vote campaign deadline has passed without reaching quorum
    PreVoteCampaignExpired,

    // The vote campaign deadline has passed without reaching quorum
    VoteCampaignExpired,

    // This member has received a pre-vote message from a leader pre-candidate
    PreVoteRequestReceived(PaxosCommand<PreVoteRequest, PreVoteResponse>),

    // This leader pre-candidate has received a pre-vote response from another member
    PreVoteResponseReceived(PreVoteResponse),

    // This member has received a vote request from a leader candidate
    VoteRequestReceived(PaxosCommand<VoteRequest, VoteResponse>),

    // This leader candidate has received a vote response from another member
    VoteResponseReceived(VoteResponse),

    // This follower has received a replicate write request from the leader
    ReplicateWriteRequestReceived(PaxosCommand<ReplicateWriteRequest, ReplicateWriteResponse>),

    // This leader has received a replicate write response from another member
    ReplicateWriteResponseReceived(ReplicateWriteResponse),
}

impl PaxosEvent {
    pub fn get_term(&self) -> Option<u32> {
        match self {
            // Internal events carry no term
            PaxosEvent::LeaderLeaseExpired
            | PaxosEvent::ElectionCountdownExpired
            | PaxosEvent::PreVoteCampaignExpired
            | PaxosEvent::VoteCampaignExpired => None,

            // Pre-vote uses a proposed next_term, not the sender's actual term — skip to avoid
            // spurious step-downs during the dry-run phase
            PaxosEvent::PreVoteRequestReceived(_) => None,

            PaxosEvent::PreVoteResponseReceived(r) => Some(r.term),
            PaxosEvent::VoteRequestReceived(cmd) => Some(cmd.get_request().term),
            PaxosEvent::VoteResponseReceived(r) => Some(r.term),
            PaxosEvent::ReplicateWriteRequestReceived(cmd) => Some(cmd.get_request().term),
            PaxosEvent::ReplicateWriteResponseReceived(r) => Some(r.term),
        }
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
