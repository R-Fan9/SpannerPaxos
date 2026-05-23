use crate::roles::{Candidate, Follower, PaxosRole, util};
use crate::state_machine::PaxosState;
use crate::{
    PaxosEvent, PaxosSharedContext, PreVoteResponse, VoteOutcome, VotePromise, VoteRejection,
    VoteRequest, VoteResponse,
};
use dashmap::{DashMap, DashSet};
use std::error::Error;
use std::sync::Arc;
use tokio::time;
use tonic::async_trait;
use uuid::Uuid;

// The state of Paxos group leader pre-candidate - Thread-safe without external locking
pub struct PreCandidate {
    // A concurrent map of Paxos group member IDs to their pre-vote responses
    pre_vote_board: DashMap<Uuid, Option<PreVoteResponse>>,

    // The deadline for the pre-vote campaign
    pre_vote_campaign_deadline: Option<time::Instant>,

    // The candidate this pre-candidate has voted for in the current term, keyed by (term, candidate_id)
    voted_for_id: Option<(u32, Uuid)>,
}

impl PreCandidate {
    pub fn new(peer_ids: &DashSet<Uuid>) -> Self {
        let pre_vote_board = DashMap::new();
        for peer_id in peer_ids.iter() {
            pre_vote_board.insert(*peer_id, None);
        }
        Self {
            pre_vote_board,
            pre_vote_campaign_deadline: None,
            voted_for_id: None,
        }
    }

    pub async fn dispatch_pre_vote(
        &mut self,
        ctx: Arc<PaxosSharedContext>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Dispatch pre-vote requests to other members to start the pre-vote campaign
        let request = ctx.create_pre_vote();
        ctx.get_dispatcher()
            .dispatch_prevote_request(request)
            .await?;

        // Set a deadline for the whole pre-vote campaign to 3 seconds
        self.pre_vote_campaign_deadline =
            Some(time::Instant::now() + time::Duration::from_millis(3000));
        Ok(())
    }

    async fn handle_pre_vote_response(
        &self,
        response: PreVoteResponse,
        ctx: Arc<PaxosSharedContext>,
    ) -> Result<Option<Result<Candidate, Follower>>, Box<dyn Error + Send + Sync>> {
        // Record the response in the board
        if !response.vote_granted {
            println!(
                "{} Info: Member {} rejected pre-vote: {}",
                ctx.log_prefix("PreCandidate"),
                response.member_id,
                response.rejection_reason()
            );
        }

        // Keep track of the response
        self.update_pre_vote_board(response);

        // Check if a quorum of members has granted the pre-vote
        if self.has_pre_vote_quorum() {
            println!(
                "{} Info: A quorum of pre-votes has been granted, transitioning to leader candidate",
                ctx.log_prefix("PreCandidate")
            );

            // Transition to a leader candidate
            let mut candidate = Candidate::new(ctx.get_peer_ids());

            // Increment the current term number
            ctx.increment_current_term();

            // Dispatch vote requests to other members to grant votes to become a leader
            candidate.dispatch_vote(ctx).await?;
            return Ok(Some(Ok(candidate)));
        }

        // Check if all responses have been received and quorum is not reached
        if self.has_all_pre_vote_responses() {
            println!(
                "{} Warning: All members responded but quorum not reached, stepping down as a follower",
                ctx.log_prefix("PreCandidate")
            );
            return Ok(Some(Err(Follower::new(None, ctx.get_event_sender()))));
        }

        // Check if the pre-vote campaign has timed out
        if self.has_pre_vote_campaign_timeout() {
            println!(
                "{} Warning: Pre-vote campaign timeout occurred, stepping down as a follower",
                ctx.log_prefix("PreCandidate")
            );
            return Ok(Some(Err(Follower::new(None, ctx.get_event_sender()))));
        }
        Ok(None)
    }

    async fn handle_vote_request(
        &mut self,
        request: VoteRequest,
        ctx: Arc<PaxosSharedContext>,
    ) -> VoteResponse {
        let current_term = ctx.get_current_term();
        let current_member_id = ctx.get_current_member_id();

        // Reject if already granted a vote in a term >= the requested term
        if let Some((voted_term, voted_id)) = self.voted_for_id {
            if voted_term >= request.term {
                return VoteResponse {
                    member_id: current_member_id,
                    term: current_term,
                    outcome: VoteOutcome::Rejection(VoteRejection {
                        current_leader_id: None,
                        member_last_log_term: None,
                        member_last_log_slot: None,
                        voted_for_id: Some(voted_id),
                    }),
                };
            }
        }

        // Reject if the proposed term number is less than or equal to the current term number
        if request.term <= current_term {
            return VoteResponse {
                member_id: current_member_id,
                term: current_term,
                outcome: VoteOutcome::Rejection(VoteRejection {
                    current_leader_id: None,
                    member_last_log_term: None,
                    member_last_log_slot: None,
                    voted_for_id: None,
                }),
            };
        }

        // Reject if the term number of the last log entry persisted by the candidate is lower than the term number of the last log persisted by the current member
        let current_last_log_term = ctx.get_last_log_term();
        if request.last_log_term < current_last_log_term {
            return VoteResponse {
                member_id: current_member_id,
                term: current_term,
                outcome: VoteOutcome::Rejection(VoteRejection {
                    current_leader_id: None,
                    member_last_log_term: Some(current_last_log_term),
                    member_last_log_slot: None,
                    voted_for_id: None,
                }),
            };
        }

        // Reject if the slot number of the last log entry persisted by the candidate is lower than the slot number of the last log persisted by the current member
        let current_last_log_slot = ctx.get_last_log_slot();
        if request.last_log_slot < current_last_log_slot {
            return VoteResponse {
                member_id: current_member_id,
                term: current_term,
                outcome: VoteOutcome::Rejection(VoteRejection {
                    current_leader_id: None,
                    member_last_log_term: Some(current_last_log_term),
                    member_last_log_slot: Some(current_last_log_slot),
                    voted_for_id: None,
                }),
            };
        }

        // Record the vote for this candidate in the current term
        self.voted_for_id = Some((request.term, request.member_id));

        VoteResponse {
            member_id: current_member_id,
            term: current_term,
            outcome: VoteOutcome::Promise(VotePromise {
                last_log_term: current_last_log_term,
                last_log_slot: current_last_log_slot,
                uncommitted_entries: ctx.get_uncommitted_entries().await,
            }),
        }
    }

    fn handle_vote_response(&self, response: VoteResponse, ctx: &PaxosSharedContext) {
        match &response.outcome {
            VoteOutcome::Promise(_) => {
                println!(
                    "{} Info: Member {} granted vote at term {}",
                    ctx.log_prefix("PreCandidate"),
                    response.member_id,
                    response.term
                );
            }
            VoteOutcome::Rejection(rejection) => {
                println!(
                    "{} Info: Member {} rejected vote: {}",
                    ctx.log_prefix("PreCandidate"),
                    response.member_id,
                    rejection.rejection_reason(response.term)
                );
            }
        }
    }

    fn update_pre_vote_board(&self, response: PreVoteResponse) {
        self.pre_vote_board
            .insert(response.member_id, Some(response));
    }

    fn has_all_pre_vote_responses(&self) -> bool {
        self.pre_vote_board
            .iter()
            .all(|entry| entry.value().is_some())
    }

    // Checks if the pre-candidate has granted a quorum of pre-votes from other members
    fn has_pre_vote_quorum(&self) -> bool {
        let num_matched = self
            .pre_vote_board
            .iter()
            .filter(|entry| matches!(entry.value(), Some(r) if r.vote_granted))
            .count();

        // + 1 accounts for the pre-candidate's implicit self-vote, which is not in the board
        (num_matched + 1) >= (self.pre_vote_board.len() + 1) / 2 + 1
    }

    // Checks if the pre-vote campaign has timed out to avoid a potential infinite hang
    fn has_pre_vote_campaign_timeout(&self) -> bool {
        self.pre_vote_campaign_deadline.is_some()
            && time::Instant::now() > self.pre_vote_campaign_deadline.unwrap()
    }
}

#[async_trait]
impl PaxosRole for PreCandidate {
    async fn handle_event(
        mut self,
        event: PaxosEvent,
        ctx: Arc<PaxosSharedContext>,
    ) -> Result<PaxosState, Box<dyn Error + Send + Sync>> {
        match event {
            PaxosEvent::LeaderLeaseExpired | PaxosEvent::ElectionCountdownExpired => {
                // Already in a leader election process, ignore these events
                Ok(PaxosState::PreCandidate(self))
            }
            PaxosEvent::PreVoteRequestReceived(pre_vote_command) => {
                let request = pre_vote_command.get_request();
                let response = util::handle_pre_vote_request(request, ctx.clone());
                pre_vote_command.send(response)?;
                Ok(PaxosState::PreCandidate(self))
            }
            PaxosEvent::PreVoteResponseReceived(response) => {
                if let Some(result) = self.handle_pre_vote_response(response, ctx.clone()).await? {
                    return match result {
                        Ok(candidate) => Ok(PaxosState::Candidate(candidate)),
                        Err(follower) => Ok(PaxosState::Follower(follower)),
                    };
                }
                Ok(PaxosState::PreCandidate(self))
            }
            PaxosEvent::VoteRequestReceived(vote_command) => {
                let request = vote_command.get_request();
                let response = self.handle_vote_request(request, ctx.clone()).await;
                vote_command.send(response)?;
                Ok(PaxosState::PreCandidate(self))
            }
            PaxosEvent::VoteResponseReceived(response) => {
                self.handle_vote_response(response, &ctx);
                Ok(PaxosState::PreCandidate(self))
            }
        }
    }
}
