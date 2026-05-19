use crate::{PaxosEvent, PaxosSharedContext, PreVoteRequest, PreVoteResponse};
use crate::roles::{PaxosRole, PreCandidate, util};
use crate::state_machine::PaxosState;
use spx_lib::count_down_clock::CountDownClock;
use std::error::Error;
use std::sync::Arc;
use tonic::async_trait;
use uuid::Uuid;

// The state of Paxos group follower
pub struct Follower {
    // The unique identifier of the leader that the follower is currently following
    current_leader_id: Option<Uuid>,

    // A count-down clock used to randomize the time between follower nodes to start leader election process
    cd_clock: CountDownClock,
}

impl Follower {
    pub fn new(current_leader_id: Option<Uuid>) -> Self {
        Self {
            current_leader_id,
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

    fn handle_pre_vote_response(&mut self, response: PreVoteResponse, ctx: &PaxosSharedContext) {
        let Some(leader_id) = util::get_active_leader(&response, ctx.get_current_term(), |received_term, local_term| received_term >= local_term) else {
            return;
        };
        println!(
            "Info: Member {} reported active leader {}, updating known leader",
            response.member_id, leader_id
        );
        self.update_current_leader_id(leader_id);
    }
}

#[async_trait]
impl PaxosRole for Follower {
    async fn handle_event(
        self,
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
        }
    }
}
