# spx

A Rust implementation of the Multi-Paxos consensus protocol, drawing on two papers:

- [Google Spanner](https://storage.googleapis.com/gweb-research2023-media/pubtools/1974.pdf) — for Multi-Paxos, leader leases powered by TrueTime, external consistency via commit wait, and `MinNextTS` for serving reads in idle groups.
- [Raft](https://raft.github.io/raft.pdf) — for the fast-track log reconciliation algorithm used to efficiently align a new leader's log with its followers.

---

## Paxos State Machine

Every node in a Paxos group runs the same event loop and transitions through four roles depending on the state of the cluster. All role transitions are driven by a single `tokio::select!` loop that multiplexes network messages, lease expiry signals, flush timers, and heartbeat timers.

```
                 ┌──────────────────────────────────────────────────┐
                 │                                                  │
                 ▼                                                  │
           ┌──────────┐  countdown expires  ┌──────────────────┐   │
  start ──►│ Follower │────────────────────►│  Pre-Candidate   │   │
           └──────────┘                     └──────────────────┘   │
                 ▲                                  │ quorum        │
                 │                                  ▼ pre-votes     │
                 │                          ┌───────────────┐       │
                 │◄─────── step-down ───────│   Candidate   │       │
                 │                          └───────────────┘       │
                 │                                  │ quorum        │
                 │                                  ▼ votes         │
                 │                          ┌───────────────┐       │
                 └──────── lease expired ───│    Leader     │───────┘
                                            └───────────────┘
```

### Follower

Every node starts as a Follower. A Follower passively replicates log entries sent by the Leader and extends its local leader lease on each valid accept request it receives — committing to not vote for a new leader until that lease expires.

When the leader lease expires, the Follower starts a **randomised countdown clock**. The randomisation spreads election attempts across the group so that multiple nodes don't race to become a candidate at the same time. Once the clock fires, the Follower promotes itself to a Pre-Candidate.

A Follower also forwards client write requests to the known Leader so clients do not need to know which node is currently leading.

### Pre-Candidate

The Pre-Candidate is a **pre-screening stage** before a real election begins. It runs a dry-run vote campaign — proposing the next term and advertising its log position — without actually incrementing its local term.

This stage exists to prevent network-partitioned nodes from disrupting the cluster. Without it, a partitioned node would increment its term on every failed election attempt. When it eventually rejoins the network, the healthy leader would see the higher term, be forced to step down, and trigger an unnecessary re-election. The Pre-Candidate avoids this: because the term is not incremented during the dry run, a partitioned node's failures are harmless.

To win the pre-vote, the Pre-Candidate must collect a quorum of votes, where each voter confirms that:

- Its own leader lease has expired (no healthy leader is currently known), and
- The Pre-Candidate's log is at least as up-to-date as the voter's.

Once a quorum of pre-votes is granted, the Pre-Candidate increments the term and transitions to a **Candidate**. If the campaign times out or all responses are received without a quorum, it steps back down to Follower.

### Candidate

The Candidate runs the real election. It broadcasts a vote request with the new term and its log position. Each voter grants a vote only if:

- Its leader lease has expired,
- The proposed term is higher than its own, and
- The Candidate's log is at least as up-to-date as the voter's.

Vote responses also carry each voter's **uncommitted log entries** — entries written to local WAL but not yet committed by a quorum. The Candidate collects these and, on winning a quorum, flushes them into its own state so that no acknowledged write is lost during leadership change.

The Candidate also seeds the **score board** — a per-follower tracking structure that records how far each follower's log has been replicated — using the log positions reported in vote responses. This score board is carried directly into the Leader role.

If the campaign times out or a quorum cannot be reached, the Candidate steps down to Follower.

### Leader

The Leader is the only role that accepts client write requests. It holds a **leader lease** that prevents other nodes from starting elections while the lease is valid. Once the lease expires, the Leader must step down.

On every client write, the Leader appends the entry to its local WAL and buffers it. Entries are not dispatched immediately; instead, the Leader sends a batch of accept requests either when the buffer reaches a configured size or when a periodic flush timer fires. This batching reduces network round-trips under write load.

Using the score board populated during the election, the Leader sends each Follower a tailored batch of entries starting from where that Follower last left off, driving all followers toward convergence with the leader's log.

When a quorum of Followers acknowledges a batch, the Leader advances the committed slot and responds to the waiting client requests. Before replying, it performs a **commit wait** to ensure external consistency (see below).

In idle periods with no client writes, the Leader sends periodic heartbeat accept requests carrying a `MinNextTS` hint to allow Followers to advance their local safe-read timestamp without needing a real write.

---

## Key Concepts

### Multi-Paxos

Classic Paxos requires two network round-trips for every log entry: a **Prepare** phase (propose a ballot term, collect promises) followed by an **Accept** phase (replicate the entry). Under write load this doubles latency.

Multi-Paxos amortises the Prepare phase. Once a Leader has won an election — meaning a quorum of peers has promised to honour its term — it skips the Prepare phase entirely for all subsequent writes and sends Accept requests directly. The Prepare phase only recurs when a new Leader is elected.

To detect Leader failure and trigger a new election, every Follower runs a countdown clock that resets whenever a valid accept request is received from the Leader. If the clock expires before the next accept request arrives, the Follower assumes the Leader has failed and starts an election. The Leader runs its own countdown clock symmetrically, resetting it whenever a quorum of accept responses is received. If the Leader's clock expires, it steps down as a Follower.

### Leader Lease

In Multi-Paxos, clock drift means two overlapping leaders can technically exist. This prevents a Leader from serving a client read directly — it would have to consult the group first to confirm it is still the leader, negating the performance benefit of having a leader at all.

The Leader Lease, as introduced in Google Spanner, solves this with a time-bounded guarantee: a Leader's lease interval is guaranteed to expire **before** a quorum of Followers will agree to elect a new one. A Follower extends its local lease expiry forward by the lease duration whenever it receives a valid accept request from the Leader. The Leader extends its own lease only when a quorum of Followers has confirmed receipt of an accept request (meaning their leases have been extended), and uses the **oldest** of those confirmed contact times as the lease base. This conservative anchor ensures the leader's lease always expires first, making it safe to serve reads without a quorum check.

**TrueTime.** The lease mechanism relies on a bounded-uncertainty clock model inspired by Spanner's TrueTime API. Rather than returning a single timestamp, the clock returns an interval `[earliest, latest]` representing the uncertainty range of the local hardware clock. By anchoring lease expiry to `earliest` (pessimistic for the sender) and follower extension to `latest` (pessimistic for the receiver), the implementation guarantees the disjointness of leader lease intervals even in the presence of clock drift, without any centralised time authority.

### Commit Wait

Commit wait enforces **external consistency**: a client will never receive a success response before the timestamp assigned to its write has passed in real-world time. Without this, a client could immediately issue a follow-up request whose timestamp might be assigned *before* the original write's timestamp — breaking causality for operations that must be strictly ordered (e.g., commenting on a photo that was just uploaded).

After a quorum commits a log entry, the Leader waits until `TrueTime.now().latest` is past the entry's assigned timestamp before replying to the client. Because `latest` is the most pessimistic bound on local clock advancement, this guarantees that no other node in the world can assign a timestamp to a subsequent write that appears to precede this one.

### Fast-Track Log Reconciliation

*Adapted from the [Raft paper](https://raft.github.io/raft.pdf), §5.3.*

When a new Leader is elected, its followers may have diverging logs: a partitioned follower may be missing entries, or may have entries from a superseded term that were never committed by a quorum.

Rather than running a full log scan upfront, reconciliation is driven lazily through the normal Accept request flow. Each accept request carries a `prev_log_slot` and `prev_log_term` anchor — the entry that must immediately precede the new entries. If the follower's log doesn't match the anchor, it responds with a **conflict hint**, and the Leader adjusts its probe point for that follower and retries. Three cases arise:

| Scenario | Follower response | Leader action |
|---|---|---|
| **Short log** — anchor slot does not exist on follower | `conflict_term = 0`, `conflict_slot = follower_last + 1` | Resend from `conflict_slot` |
| **Term mismatch** — anchor slot exists but with a different term | `conflict_term = local_term`, `conflict_slot = first slot of that term` | If leader has that term: resend from just after leader's last slot for that term. If not: skip to `conflict_slot` |
| **Match** — anchor found and terms agree | Truncate any conflicting entries ahead of the anchor, append new entries to local WAL, reply `success = true` | Update score board |

Reporting the **first** slot of the conflicting term (not the last) is critical: it lets the Leader bypass the entire dirty term in one step, minimising round-trips.

### MinNextTS

Followers advance their safe-read timestamp (`t_safe`) when they commit a log entry, setting it to that entry's timestamp. A Follower can serve a strong read at timestamp `T` only when `t_safe >= T`. In an idle group with no client writes, `t_safe` stalls indefinitely, blocking reads.

`MinNextTS(n)` is a promise broadcast by the Leader: *the timestamp assigned to log entry `n+1` will be strictly greater than `MinNextTS(n)`*. In other words, the Leader declares that no write will occur until this time has passed. A Follower that has caught up with the Leader (i.e., has committed all entries the Leader has committed) can safely advance its local `t_safe` to `MinNextTS(n) - 1ε` without waiting for a new entry.

The Leader broadcasts `MinNextTS` via periodic heartbeat accept requests during idle periods. The value is capped at the leader lease expiry — the Leader cannot make promises about timestamps it may not be around to honour.
