use crate::roles::{PaxosRole, PreCandidate};
use crate::state_machine::PaxosState;
use crate::{
    PaxosEvent, PaxosSharedContext, PreVoteRequest, PreVoteResponse, VoteOutcome, VotePromise,
    VoteRejection, VoteRequest, VoteResponse,
};
use spx_lib::count_down_clock::CountDownClock;
use spx_lib::worker_runner::WorkerRunner;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use tonic::async_trait;
use uuid::Uuid;

// The state of Paxos group follower
pub struct Follower {
    // The unique identifier of the leader that the follower is currently following
    current_leader_id: Option<Uuid>,

    // The candidate this follower has voted for in the current term, keyed by (term, candidate_id)
    voted_for_id: Option<(u32, Uuid)>,

    // A count-down clock worker that fires ElectionCountdownExpired after a random delay
    cd_runner: WorkerRunner<CountDownClock>,

    // Cancellation token scoped to this follower's countdown; canceled when the follower steps down
    cd_token: CancellationToken,
}

impl Follower {
    pub fn new(current_leader_id: Option<Uuid>, event_tx: Sender<PaxosEvent>) -> Self {
        let cd_clock = CountDownClock::new(1000, move || {
            if let Err(e) = event_tx.try_send(PaxosEvent::ElectionCountdownExpired) {
                eprintln!("Error: Failed to send ElectionCountdownExpired event: {e}");
            }
        });
        Self {
            current_leader_id,
            voted_for_id: None,
            cd_runner: WorkerRunner::new(cd_clock),
            cd_token: CancellationToken::new(),
        }
    }

    pub fn get_current_leader_id(&self) -> Option<Uuid> {
        self.current_leader_id
    }

    pub fn update_current_leader_id(&mut self, leader_id: Uuid) {
        self.current_leader_id = Some(leader_id);
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

        // Reset the countdown so a spurious ElectionCountdownExpired doesn't fire while a
        // candidate is actively running an election
        if let Ok(clock) = self.cd_runner.get_worker() {
            clock.reset();
        }

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

    async fn handle_pre_vote_response(
        &mut self,
        response: PreVoteResponse,
        ctx: &PaxosSharedContext,
    ) {
        if !ctx.is_leader_lease_expired().await {
            return;
        }

        let Some(leader_id) =
            response.try_get_leader(|received_term| received_term >= ctx.get_current_term())
        else {
            return;
        };
        println!(
            "[Follower {}] Info: Member {} reported active leader {} at term {} in pre-vote response, updating known leader",
            ctx.get_current_member_id(), response.member_id, leader_id, response.term
        );
        self.update_current_leader_id(leader_id);
    }

    async fn handle_vote_response(&mut self, response: VoteResponse, ctx: &PaxosSharedContext) {
        if !ctx.is_leader_lease_expired().await {
            return;
        }

        let Some(leader_id) =
            response.try_get_leader(|received_term| received_term >= ctx.get_current_term())
        else {
            return;
        };
        println!(
            "[Follower {}] Info: Member {} reported active leader {} at term {} in vote response, updating known leader",
            ctx.get_current_member_id(), response.member_id, leader_id, response.term
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
                println!(
                    "[Follower {}] Info: leader lease expired, starting election countdown",
                    ctx.get_current_member_id()
                );
                self.cd_runner.start(self.cd_token.clone()).await?;
                Ok(PaxosState::Follower(self))
            }
            PaxosEvent::ElectionCountdownExpired => {
                println!(
                    "[Follower {}] Info: election countdown expired, transitioning to pre-candidate",
                    ctx.get_current_member_id()
                );
                let mut pre_candidate = PreCandidate::new(ctx.get_peer_ids());
                pre_candidate.dispatch_pre_vote(ctx).await?;
                Ok(PaxosState::PreCandidate(pre_candidate))
            }
            PaxosEvent::PreVoteRequestReceived(pre_vote_command) => {
                let request = pre_vote_command.get_request();
                let response = self.handle_pre_vote_request(request, ctx.clone()).await;
                pre_vote_command.send(response)?;
                Ok(PaxosState::Follower(self))
            }
            PaxosEvent::PreVoteResponseReceived(response) => {
                self.handle_pre_vote_response(response, &ctx).await;
                Ok(PaxosState::Follower(self))
            }
            PaxosEvent::VoteRequestReceived(vote_command) => {
                let request = vote_command.get_request();
                let mut follower = self;
                let response = follower.handle_vote_request(request, ctx.clone()).await;
                vote_command.send(response)?;
                Ok(PaxosState::Follower(follower))
            }
            PaxosEvent::VoteResponseReceived(response) => {
                self.handle_vote_response(response, &ctx).await;
                Ok(PaxosState::Follower(self))
            }
        }
    }
}

impl Drop for Follower {
    fn drop(&mut self) {
        // CancellationToken is Arc-backed, so dropping it does not cancel the underlying task.
        // Explicitly cancel here so the countdown background task is always stopped when the
        // follower transitions to any other state.
        self.cd_token.cancel();
    }
}
