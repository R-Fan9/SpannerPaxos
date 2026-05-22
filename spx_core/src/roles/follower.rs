use crate::roles::{PaxosRole, PreCandidate, util};
use crate::state_machine::PaxosState;
use crate::{
    PaxosEvent, PaxosSharedContext, PreVoteRequest, PreVoteResponse, VoteOutcome, VotePromise,
    VoteRejection, VoteRequest, VoteResponse,
};
use spx_lib::count_down_clock::CountDownClock;
use std::error::Error;
use std::sync::Arc;
use tonic::async_trait;
use uuid::Uuid;

// The state of Paxos group follower
pub struct Follower {
    // The unique identifier of the leader that the follower is currently following
    current_leader_id: Option<Uuid>,

    // The candidate this follower has voted for in the current term, keyed by (term, candidate_id)
    voted_for_id: Option<(u32, Uuid)>,

    // A count-down clock used to randomize the time between follower nodes to start leader election process
    cd_clock: CountDownClock,
}

impl Follower {
    pub fn new(current_leader_id: Option<Uuid>) -> Self {
        Self {
            current_leader_id,
            voted_for_id: None,
            cd_clock: CountDownClock::new(1000),
        }
    }
    pub fn get_current_leader_id(&self) -> Option<Uuid> {
        self.current_leader_id
    }

    pub fn update_current_leader_id(&mut self, leader_id: Uuid) {
        self.current_leader_id = Some(leader_id);
    }

    async fn handle_expired_leader_lease(
        &self,
        ctx: Arc<PaxosSharedContext>,
    ) -> Result<PreCandidate, Box<dyn Error + Send + Sync>> {
        // Start and await the count-down clock to reach zero
        self.cd_clock.start().await;

        // Count-down passed, transition to a leader pre-candidate
        println!(
            "Info: Follower {} count-down clock reached zero, transitioning to pre-candidate",
            ctx.get_current_member_id()
        );
        let mut pre_candidate = PreCandidate::new(ctx.get_peer_ids());

        // Dispatch pre-vote requests to other members to qualify to become a leader candidate
        pre_candidate.dispatch_pre_vote(ctx).await?;
        Ok(pre_candidate)
    }

    async fn handle_pre_vote_request(
        &self,
        request: PreVoteRequest,
        ctx: Arc<PaxosSharedContext>,
    ) -> PreVoteResponse {
        let current_term = ctx.get_current_term();
        let current_member_id = ctx.get_current_member_id();
        let current_leader_id = self.current_leader_id;

        // Reject if the leader lease has not expired yet
        if !ctx.is_leader_lease_expired().await {
            return PreVoteResponse {
                term: current_term,
                member_id: current_member_id,
                vote_granted: false,
                current_leader_id,
                member_last_log_term: None,
                member_last_log_slot: None,
            };
        }

        // Reject if the next term number proposal is less than or equal to the current term number
        if request.next_term <= current_term {
            return PreVoteResponse {
                term: current_term,
                member_id: current_member_id,
                vote_granted: false,
                current_leader_id,
                member_last_log_term: None,
                member_last_log_slot: None,
            };
        }

        // Reject if the term number of the last log entry persisted by the pre-candidate is lower than the term number of the last log persisted by the current member (node)
        let current_last_log_term = ctx.get_last_log_term();
        if request.last_log_term < current_term {
            return PreVoteResponse {
                term: current_term,
                member_id: current_member_id,
                vote_granted: false,
                current_leader_id,
                member_last_log_term: Some(current_last_log_term),
                member_last_log_slot: None,
            };
        }

        // Reject if the slot number of the last log entry persisted by the pre-candidate is lower than the slot number of the last log persisted by the current member (node)
        let current_last_log_slot = ctx.get_last_log_slot();
        if request.last_log_slot < current_last_log_slot {
            return PreVoteResponse {
                term: current_term,
                member_id: current_member_id,
                vote_granted: false,
                current_leader_id,
                member_last_log_term: Some(current_last_log_term),
                member_last_log_slot: Some(current_last_log_slot),
            };
        }

        PreVoteResponse {
            term: current_term,
            member_id: current_member_id,
            vote_granted: true,
            current_leader_id: None,
            member_last_log_term: None,
            member_last_log_slot: None,
        }
    }

    async fn handle_vote_request(
        &mut self,
        request: VoteRequest,
        ctx: Arc<PaxosSharedContext>,
    ) -> VoteResponse {
        let current_term = ctx.get_current_term();
        let current_member_id = ctx.get_current_member_id();
        let current_leader_id = self.current_leader_id;

        // Reject if this follower has already granted a vote in a term >= the requested term
        if let Some((voted_term, voted_id)) = self.voted_for_id {
            if voted_term >= request.next_term {
                return VoteResponse {
                    member_id: current_member_id,
                    term: current_term,
                    outcome: VoteOutcome::Rejection(VoteRejection {
                        current_leader_id,
                        member_last_log_term: None,
                        member_last_log_slot: None,
                        voted_for_id: Some(voted_id),
                    }),
                };
            }
        }

        // Reject if the leader lease has not expired yet
        if !ctx.is_leader_lease_expired().await {
            return VoteResponse {
                member_id: current_member_id,
                term: current_term,
                outcome: VoteOutcome::Rejection(VoteRejection {
                    current_leader_id,
                    member_last_log_term: None,
                    member_last_log_slot: None,
                    voted_for_id: None,
                }),
            };
        }

        // Reject if the proposed term number is less than or equal to the current term number
        if request.next_term <= current_term {
            return VoteResponse {
                member_id: current_member_id,
                term: current_term,
                outcome: VoteOutcome::Rejection(VoteRejection {
                    current_leader_id,
                    member_last_log_term: None,
                    member_last_log_slot: None,
                    voted_for_id: None,
                }),
            };
        }

        // Reject if the term number of the last log entry persisted by the candidate is lower than the term number of the last log persisted by the current member (node)
        let current_last_log_term = ctx.get_last_log_term();
        if request.last_log_term < current_last_log_term {
            return VoteResponse {
                member_id: current_member_id,
                term: current_term,
                outcome: VoteOutcome::Rejection(VoteRejection {
                    current_leader_id,
                    member_last_log_term: Some(current_last_log_term),
                    member_last_log_slot: None,
                    voted_for_id: None,
                }),
            };
        }

        // Reject if the slot number of the last log entry persisted by the candidate is lower than the slot number of the last log persisted by the current member (node)
        let current_last_log_slot = ctx.get_last_log_slot();
        if request.last_log_slot < current_last_log_slot {
            return VoteResponse {
                member_id: current_member_id,
                term: current_term,
                outcome: VoteOutcome::Rejection(VoteRejection {
                    current_leader_id,
                    member_last_log_term: Some(current_last_log_term),
                    member_last_log_slot: Some(current_last_log_slot),
                    voted_for_id: None,
                }),
            };
        }

        // Record the vote for this candidate in the current term
        self.voted_for_id = Some((request.next_term, request.member_id));

        // Reset the count-down clock to suppress spurious leader elections while a candidate is active
        self.cd_clock.reset();

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

    fn handle_pre_vote_response(&mut self, response: PreVoteResponse, ctx: &PaxosSharedContext) {
        let Some(leader_id) =
            response.try_get_leader(|received_term| received_term >= ctx.get_current_term())
        else {
            return;
        };
        println!(
            "Info: Member {} reported active leader {} at term {}, updating known leader",
            response.member_id, leader_id, response.term
        );
        self.update_current_leader_id(leader_id);
    }
}

#[async_trait]
impl PaxosRole for Follower {
    async fn handle_event(
        mut self,
        event: PaxosEvent,
        ctx: Arc<PaxosSharedContext>,
    ) -> Result<PaxosState, Box<dyn Error + Send + Sync>> {
        match event {
            PaxosEvent::LeaderLeaseExpired => {
                if self.cd_clock.has_started() {
                    // The leader election count-down clock has already started, skip
                    return Ok(PaxosState::Follower(self));
                }
                let pre_candidate = self.handle_expired_leader_lease(ctx.clone()).await?;
                Ok(PaxosState::PreCandidate(pre_candidate))
            }
            PaxosEvent::PreVoteRequestReceived(pre_vote_command) => {
                let request = pre_vote_command.get_request();
                let response = self.handle_pre_vote_request(request, ctx.clone()).await;
                pre_vote_command.send(response)?;
                Ok(PaxosState::Follower(self))
            }
            PaxosEvent::PreVoteResponseReceived(response) => {
                self.handle_pre_vote_response(response, &ctx);
                Ok(PaxosState::Follower(self))
            }
            PaxosEvent::VoteRequestReceived(vote_command) => {
                let request = vote_command.get_request();
                let mut follower = self;
                let response = follower.handle_vote_request(request, ctx.clone()).await;
                vote_command.send(response)?;
                Ok(PaxosState::Follower(follower))
            }
            PaxosEvent::VoteResponseReceived(_) => {
                todo!()
            }
        }
    }
}
