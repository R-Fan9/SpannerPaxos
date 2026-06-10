use crate::{
    AcceptRequest, AcceptResponse, ClientWriteRequest, ClientWriteResponse, PaxosCommand,
    PreVoteRequest, PreVoteResponse, VoteRequest, VoteResponse,
};
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

    // The write flush timer fired; the leader should dispatch any buffered writes to followers
    WriteFlushTimerFired,

    // The periodic accept timeout check fired; the leader should inspect each follower's
    // in-flight batch queue and clear any batches that have been waiting too long
    AcceptTimeoutCheckFired,

    // No client writes have arrived for 8 seconds; the leader should broadcast a heartbeat
    // accept request with an updated min_next_ts to advance t_safe on all followers
    HeartbeatTimerFired,

    // This member has received a pre-vote message from a leader pre-candidate
    PreVoteRequestReceived(PaxosCommand<PreVoteRequest, PreVoteResponse>),

    // This leader pre-candidate has received a pre-vote response from another member
    PreVoteResponseReceived(PreVoteResponse),

    // This member has received a vote request from a leader candidate
    VoteRequestReceived(PaxosCommand<VoteRequest, VoteResponse>),

    // This leader candidate has received a vote response from another member
    VoteResponseReceived(VoteResponse),

    // This follower has received an accept (AppendEntries) request from the leader
    AcceptRequestReceived(PaxosCommand<AcceptRequest, AcceptResponse>),

    // This leader has received an accept response from a follower
    AcceptResponseReceived(AcceptResponse),

    // This leader has received a write request from a client
    ClientWriteRequestReceived(PaxosCommand<ClientWriteRequest, ClientWriteResponse>),
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
            PaxosEvent::AcceptRequestReceived(cmd) => Some(cmd.get_request().term),
            PaxosEvent::AcceptResponseReceived(r) => Some(r.term),
            // Client writes carry no Paxos term
            PaxosEvent::ClientWriteRequestReceived(_) => None,
            PaxosEvent::WriteFlushTimerFired => None,
            PaxosEvent::AcceptTimeoutCheckFired => None,
            PaxosEvent::HeartbeatTimerFired => None,
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
