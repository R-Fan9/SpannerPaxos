use crate::roles::{PaxosRole, PreCandidate};
use crate::state_machine::PaxosState;
use crate::{
    PaxosEvent, PaxosSharedContext, PreVoteRequest, PreVoteResponse, VoteOutcome, VotePromise,
    VoteRejection, VoteRequest, VoteResponse,
};
use spx_lib::count_down_clock::CountDownClock;
use spx_lib::true_time::TrueTime;
use std::error::Error;
use tonic::async_trait;
use uuid::Uuid;

struct VoteRecord {
    term: u32,
    candidate_id: Uuid,
}

// The state of Paxos group follower
pub struct Follower {
    // The unique identifier of the leader that the follower is currently following
    current_leader_id: Option<Uuid>,

    // The most recent vote granted by this follower, including when the vote lease expires
    vote_record: Option<VoteRecord>,

    // A count-down clock that fires ElectionCountdownExpired after a random delay
    election_cd_clock: CountDownClock,
}

impl Follower {
    pub fn new(current_leader_id: Option<Uuid>) -> Self {
        Self {
            current_leader_id,
            vote_record: None,
            election_cd_clock: CountDownClock::new(),
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
        ctx: &PaxosSharedContext,
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
        ctx: &PaxosSharedContext,
    ) -> VoteResponse {
        let current_term = ctx.get_current_term();
        let current_member_id = ctx.get_current_member_id();
        let current_leader_id = self.current_leader_id;

        // Reject if this follower has already granted a vote in a term >= the requested term
        if let Some(ref record) = self.vote_record {
            if record.term >= request.term {
                return VoteResponse {
                    member_id: current_member_id,
                    term: current_term,
                    outcome: VoteOutcome::Rejection(VoteRejection {
                        current_leader_id,
                        member_last_log_term: None,
                        member_last_log_slot: None,
                        voted_for_id: Some(record.candidate_id),
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
        if request.term <= current_term {
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

        // Record the vote to prevent double-voting within the same term
        self.vote_record = Some(VoteRecord {
            term: request.term,
            candidate_id: request.member_id,
        });

        // Treat the vote as a leader lease: block new elections and votes until the lease expires
        ctx.update_leader_lease_expiry_time(TrueTime::now().latest + ctx.get_lease_length())
            .await;

        VoteResponse {
            member_id: current_member_id,
            term: current_term,
            outcome: VoteOutcome::Promise(VotePromise {
                last_log_term: current_last_log_term,
                last_log_slot: current_last_log_slot,
                uncommitted_entries: ctx.get_uncommitted_logs().values().cloned().collect(),
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
            "{} Info: Member {} reported active leader {} at term {} in pre-vote response, updating known leader",
            ctx.log_prefix("Follower"),
            response.member_id,
            leader_id,
            response.term
        );
        self.update_current_leader_id(leader_id);
    }

    async fn handle_election_countdown_expired(
        &self,
        ctx: &mut PaxosSharedContext,
    ) -> Result<Option<PreCandidate>, Box<dyn Error + Send + Sync>> {
        // Don't promote while the leader lease is still active; the countdown task has already
        // finished so no new task is spawned here. LeaderLeaseExpired will restart the count-down clock
        if !ctx.is_leader_lease_expired().await {
            return Ok(None);
        }

        println!(
            "{} Info: election countdown expired, transitioning to pre-candidate",
            ctx.log_prefix("Follower")
        );
        let mut pre_candidate = PreCandidate::new(ctx);
        pre_candidate.dispatch_pre_vote(ctx).await?;
        Ok(Some(pre_candidate))
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
            "{} Info: Member {} reported active leader {} at term {} in vote response, updating known leader",
            ctx.log_prefix("Follower"),
            response.member_id,
            leader_id,
            response.term
        );
        self.update_current_leader_id(leader_id);
    }
}

#[async_trait]
impl PaxosRole for Follower {
    async fn handle_event(
        mut self,
        event: PaxosEvent,
        ctx: &mut PaxosSharedContext,
    ) -> Result<PaxosState, Box<dyn Error + Send + Sync>> {
        match event {
            PaxosEvent::PreVoteCampaignExpired | PaxosEvent::VoteCampaignExpired => {
                // Not in an election campaign, ignore these events
                Ok(PaxosState::Follower(self))
            }
            PaxosEvent::LeaderLeaseExpired => {
                println!(
                    "{} Info: leader lease expired, starting election countdown",
                    ctx.log_prefix("Follower")
                );
                let event_tx = ctx.get_event_sender();
                self.election_cd_clock.start_random(1000, move || {
                    if let Err(e) = event_tx.try_send(PaxosEvent::ElectionCountdownExpired) {
                        eprintln!("Error: Failed to send ElectionCountdownExpired event: {e}");
                    }
                });
                Ok(PaxosState::Follower(self))
            }
            PaxosEvent::ElectionCountdownExpired => {
                if let Some(pre_candidate) = self.handle_election_countdown_expired(ctx).await? {
                    return Ok(PaxosState::PreCandidate(pre_candidate));
                }
                Ok(PaxosState::Follower(self))
            }
            PaxosEvent::PreVoteRequestReceived(pre_vote_command) => {
                let request = pre_vote_command.get_request();
                let response = self.handle_pre_vote_request(request, ctx).await;
                pre_vote_command.send(response)?;
                Ok(PaxosState::Follower(self))
            }
            PaxosEvent::PreVoteResponseReceived(response) => {
                self.handle_pre_vote_response(response, ctx).await;
                Ok(PaxosState::Follower(self))
            }
            PaxosEvent::VoteRequestReceived(vote_command) => {
                let request = vote_command.get_request();
                let response = self.handle_vote_request(request, ctx).await;
                vote_command.send(response)?;
                Ok(PaxosState::Follower(self))
            }
            PaxosEvent::VoteResponseReceived(response) => {
                self.handle_vote_response(response, ctx).await;
                Ok(PaxosState::Follower(self))
            }
            PaxosEvent::AcceptRequestReceived(_command) => {
                // TODO: handle accept (AppendEntries) request from the leader
                todo!()
            }
            PaxosEvent::AcceptResponseReceived(_response) => {
                // Accept responses are handled by the leader, not follower
                Ok(PaxosState::Follower(self))
            }
        }
    }
}

impl Drop for Follower {
    fn drop(&mut self) {
        // Explicitly cancel so the countdown background task is always stopped when the
        // follower transitions to any other state.
        self.election_cd_clock.cancel();
    }
}
