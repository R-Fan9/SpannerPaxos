use crate::roles::{PaxosRole, PreCandidate};
use crate::state_machine::PaxosState;
use crate::{
    AcceptRequest, AcceptResponse, ClientWriteResponse, ConflictHint, LogEntry, PaxosEvent,
    PaxosSharedContext, PreVoteRequest, PreVoteResponse, VoteOutcome, VotePromise, VoteRejection,
    VoteRequest, VoteResponse,
};
use spx_lib::count_down_clock::CountDownClock;
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
        ctx.extend_leader_lease_expiry_time().await;

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

    async fn handle_accept_request(
        &mut self,
        request: AcceptRequest,
        ctx: &mut PaxosSharedContext,
    ) -> AcceptResponse {
        let current_term = ctx.get_current_term();
        let current_member_id = ctx.get_current_member_id();
        let t_send = request.t_send;

        // Ignore stale requests from leaders in an older term
        if request.term < current_term {
            println!(
                "{} Info: Ignoring accept request from {} with stale term {} (current: {})",
                ctx.log_prefix("Follower"),
                request.leader_id,
                request.term,
                current_term
            );
            return AcceptResponse {
                member_id: current_member_id,
                term: current_term,
                success: false,
                last_written_slot: 0,
                conflict_hint: None,
                echoed_t_send: t_send,
            };
        }

        // Track the current leader from every accept request
        self.update_current_leader_id(request.leader_id);

        let prev_log_slot = request.prev_log_slot;

        let response = if prev_log_slot > 0 && !ctx.get_wal().has_entry(prev_log_slot) {
            // Short log — follower doesn't have the anchor slot
            AcceptResponse {
                member_id: current_member_id,
                term: current_term,
                success: false,
                last_written_slot: 0,
                conflict_hint: Some(ConflictHint {
                    // conflict_term 0 signals the leader that the follower's log is simply too short
                    conflict_term: 0,
                    // point the leader to the first slot the follower is missing
                    conflict_first_slot: ctx.get_last_log_slot() + 1,
                }),
                echoed_t_send: t_send,
            }
        } else if prev_log_slot > 0
            && ctx
                .get_wal()
                .get_term(prev_log_slot)
                .expect("entry confirmed present by has_entry")
                != request.prev_log_term
        {
            // Term mismatch at the anchor slot
            let local_term = ctx
                .get_wal()
                .get_term(prev_log_slot)
                .expect("entry confirmed present by has_entry");
            let conflict_first_slot = ctx
                .get_wal()
                .find_lowest_slot_for_term(local_term)
                .expect("term just read from WAL must exist");
            AcceptResponse {
                member_id: current_member_id,
                term: current_term,
                success: false,
                last_written_slot: 0,
                conflict_hint: Some(ConflictHint {
                    conflict_term: local_term,
                    // first slot of the conflicting term so the leader can skip the whole term
                    conflict_first_slot,
                }),
                echoed_t_send: t_send,
            }
        } else {
            // Anchor matches — truncate conflicts and append new entries
            ctx.get_wal_mut().truncate_from(prev_log_slot + 1);
            let mut last_written_slot = prev_log_slot;
            for log_entry in &request.entries {
                ctx.get_wal_mut()
                    .append(log_entry.slot, log_entry.term, log_entry.entry.clone());

                ctx.get_uncommitted_logs_mut().insert(log_entry.slot, LogEntry {
                    term: log_entry.term,
                    slot: log_entry.slot,
                    entry: log_entry.entry.clone(),
                });

                last_written_slot = log_entry.slot;
            }

            // Advance the local log position to what was just written
            if let Some(last) = request.entries.last() {
                ctx.set_last_log_slot(last.slot);
                ctx.set_last_log_term(last.term);
            }

            // Advance committed slot and evict entries that are now committed
            let new_committed_slot = request.leader_commit_slot.min(last_written_slot);
            if new_committed_slot > ctx.get_committed_slot() {
                ctx.set_committed_slot(new_committed_slot);
                ctx.get_uncommitted_logs_mut()
                    .retain(|&slot, _| slot > new_committed_slot);
            }

            AcceptResponse {
                member_id: current_member_id,
                term: current_term,
                success: true,
                last_written_slot,
                conflict_hint: None,
                echoed_t_send: t_send,
            }
        };

        // Extend the leader lease on every valid accept request, regardless of outcome
        ctx.extend_leader_lease_expiry_time().await;

        response
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
            PaxosEvent::AcceptRequestReceived(command) => {
                let request = command.get_request();
                let response = self.handle_accept_request(request, ctx).await;
                command.send(response)?;
                Ok(PaxosState::Follower(self))
            }
            PaxosEvent::AcceptResponseReceived(response) => {
                println!(
                    "{} Info: Ignoring accept response from {} — success: {}, term: {}, last_written_slot: {}{}",
                    ctx.log_prefix("Follower"),
                    response.member_id,
                    response.success,
                    response.term,
                    response.last_written_slot,
                    response.conflict_hint.as_ref().map_or(String::new(), |h| {
                        format!(
                            ", conflict_term: {}, conflict_first_slot: {}",
                            h.conflict_term, h.conflict_first_slot
                        )
                    })
                );
                Ok(PaxosState::Follower(self))
            }
            PaxosEvent::ClientWriteRequestReceived(command) => {
                let _ = command.send(ClientWriteResponse {
                    success: false,
                    error: Some("not the leader".to_string()),
                });
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
