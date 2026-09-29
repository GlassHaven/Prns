# Authorization commit and recovery

Status: proposed implementation contract, not implemented assurance. This follows
the [rollback power-loss finding](measurements/grant-persistence-cuts.md#open-finding-rollback-intent-is-volatile).
The existing campaign remains a characterization until production consumers use
the new contract and the same cuts recover the required authority.

## What the audit establishes

| Owner | Current order | Consequence |
| --- | --- | --- |
| Embassy pairing persistence | Store candidate, activate, settle pairing, possibly store prior | Power loss before rollback commit can restore a rejected candidate. Demonstrated with real flash-owner cuts. |
| Tokio pairing persistence | Store candidate, activate, settle pairing, possibly store prior | The same ordering exists in source. No host crash campaign has demonstrated its cut points yet. |
| Tokio controller-grant management | Store candidate, check caller, activate, send outcome, possibly store prior | Caller cancellation or lost completion can initiate rollback after candidate storage. Source-audited, not crash-tested. |
| Shared target pairing engine | Validate deadline, sign completion, dispatch completion in one settlement | Persistence cannot currently obtain a successful preparation decision separately from sending success. |
| Shared controller pairing engine | Consume persisted result and retire the pairing link | A target-access transaction also needs attempt ownership through durable completion. |

The source owners are:

- `prns-core/src/engine/remote_control/target_pairing_authorization.rs`
- `prns-core/src/engine/commands/remote_control_controller_pairing.rs`
- `prns-core/src/remote_control/pairing/target_pairing/state.rs`
- `prns-runtime/impls/embassy/src/runtime/remote_control_pairing_persistence.rs`
- `prns-runtime/impls/embassy/src/runtime/remote_control_pairing_authorizations.rs`
- `prns-runtime/impls/tokio/src/runtime/remote_control_pairing_persistence.rs`
- `prns-runtime/impls/tokio/src/runtime/remote_control_controller_grants.rs`

Snapshot atomicity is not transaction atomicity. Neither choosing an older valid
snapshot on boot nor persisting a rollback marker only after rejection closes the
window between candidate commit and that marker. Moving the final store after the
existing settlement is also insufficient: settlement can already send success.

## Commit contract

Use a single local durable commit point. Before it, reboot recovers the prior
authorization; after it, reboot recovers the candidate. A response is evidence of
an already committed operation, not the event that commits it.

1. Validate the desired mutation and reserve its transaction owner without
   changing live authority. Preserve the complete prior and candidate values.
2. Prepare the pairing completion in shared core: check the exact attempt,
   permissions and deadline, and perform fallible signing. Do not dispatch a
   success response. A rejection here cannot leave candidate authority durable.
3. Hold the prepared attempt while committing storage. Cancellation and timeout
   must not independently discard that owner while its I/O may still commit.
4. Establish the durable decision, then activate its exact candidate and release
   the prepared completion for dispatch. No success is emitted before durability.
5. A post-commit dispatch failure is an undelivered committed outcome, not an
   implicit revocation. Retrying delivery cannot repeat or reverse the mutation.

The readiness check is the deadline boundary for admission to commit; storage
acknowledgement may arrive later. This is an intentional change from today's
post-store deadline check and needs an explicit shared-core state and boundary
tests, not a relaxed comparison inside an adapter. The prepared phase must not
renew the original window or allow an expired attempt to enter it.

Caller cancellation has the same boundary. Before commit starts it may abort;
once a write can have committed, cancellation alone cannot promise rollback.
Any explicit subsequent revocation is a new transaction. Preserve administrator
authorization rules and unchanged-operation behavior throughout this change.

Durable commitment and successful delivery to another device cannot be made one
atomic operation. A crash after commit but before delivery may leave one peer
uncertain. This design does not claim distributed exactly-once pairing or reboot-
persistent completion retransmission. Existing completion-retention and signed
transcript behavior must be checked before wiring the new prepared phase.

## Storage design gate

Do not add an envelope or another full snapshot buffer merely to name a state.
First implement and test the shared prepare/commit boundary. If all rejectable
decisions can move before the existing atomic snapshot replacement, that snapshot
can remain the durable commit record and existing boot formats can remain valid.

An alternative staged-record design is required if a candidate must be written
before a rejectable decision remains. In that design:

- A pending candidate is not boot authority. Recovery retains the complete prior
  committed table until an explicit, matching commit decision is durable.
- The decision binds the authorization region and exact candidate, not just a
  volatile pairing attempt ID that may be reused after reboot.
- Compaction must retain enough information to recover the committed table while
  a transaction is pending. It must not promote the cached candidate accidentally.
- Legacy snapshots remain committed snapshots; unreleased pending records must
  never be interpreted as legacy authority. Unknown versions fail closed.
- Space admission reserves what is needed to finish or safely abandon a pending
  transaction before accepting it. Daily compaction limits still apply.

Choose between these designs before publishing a new storage format. A pending
record alone does not fix a success response dispatched before durable commit.

## I/O outcomes and ownership

Use typed committed, not-committed and indeterminate outcomes where the backend
can distinguish them. A lost acknowledgement is not proof of a failed write.
Do not release transaction ownership or admit another authorization mutation
while an indeterminate result could later commit. Recovery/read-back must resolve
the exact candidate versus prior durable value; an unresolved outcome must not
be reported as success or successful rollback.

For Tokio, `RemoteControlAuthorizationPersistence::store` runs a blocking task.
Dropping the awaiting future does not establish that the underlying task stopped.
Tests must hold and release that worker explicitly rather than equating future
cancellation with power removal. Also audit background snapshot flush ownership
so it cannot overwrite a decision using a stale live table.

`FileStore::store` currently syncs the staging file and renames it, but does not
sync the parent directory. Process-restart tests and NOR-model cuts must not be
presented as proof of host filesystem power-loss durability. Define and verify
the applicable filesystem durability contract separately, including supported
non-Unix hosts, before using a stronger claim in the transaction API.

For Embassy, keep the bounded exchange, exact completion and compaction wear
budget. Audit the owner's cached snapshots and boot restoration together with
its transaction adapter. A live activation inconsistency after durable commit
must fail closed and recover from the durable decision, not silently rewrite
the prior table as though commitment never happened.

## Implementation and evidence slices

1. **Shared preparation boundary.** Split target pairing preparation from
   completion dispatch using typed attempt-bound states. Cover signing failure,
   attempt mismatch, exact deadline, timeout while reserved, duplicate completion
   and dispatch failure. Include the controller-side target-access lifecycle.
   Do not change shipping adapters until the new contract is usable end to end.
2. **Runtime integration.** Apply the same commit policy to Embassy and Tokio
   pairing and grant management. Retain bounded admission, explicit ownership,
   exact live activation and no success before commit. Resolve indeterminate I/O
   and cancellation without permitting a concurrent mutation to race recovery.
3. **Recovery campaigns.** Change the Embassy characterization's expected tables
   only after the production boundary changes. Cut before preparation, throughout
   storage, before activation, before dispatch and during retry. Check prior or
   candidate by the durable decision, not by whether the caller saw success.
   Add host worker-cancellation/restart cases and both authorization regions.
4. **Whole-node assurance.** Replace scripted settlement with real engine
   preparation/dispatch; exercise pairing deadlines and lost responses through
   controlled nodes. Keep codec/flash tests as narrower independent evidence.
   Run affected registered suites, focused proofs, firmware builds and resource
   contracts before claiming the production change fits supported boards.

Each slice must retain whole-value grant/access assertions, including unrelated
administrators, and compare repeated fresh boots. Fault campaigns must cover
first grants, replacements and revocations, not only a one-entry permission
change. RAM/stack, record capacity, flash writes and compaction pressure require
before/after evidence; this proposal claims no free resource cost.

Production impact of this design slice: none. It records the cross-runtime audit,
proposed observable semantics and implementation gates; it neither repairs the
open crash window nor adds a new runtime dependency or persisted format.

## Implementation checkpoint: shared target preparation

The first implementation slice adds `prepare_authorization` to the shared target
pairing state machine. Its typed outcome reports readiness, deadline refusal,
signing failure, attempt mismatch or absence of authorization work. Readiness
retains the signed completion internally without returning a deliverable response.
The state remains authorizing until persistence settles.

Preparation checks the existing deadline strictly. Once prepared, retries use the
retained signature, and timeout, rejection and link loss cannot consume the
reserved authorization. A correlated storage failure can still abort it. A
successful persistence settlement can release the exact completion after the
original deadline without re-signing. Unprepared callers retain the legacy
post-store deadline/signing behavior; they have not silently adopted this policy.

Six focused tests cover preparation, exact deadline boundaries, signing failure,
attempt correlation, failure settlement and non-authorizing states. The existing
competing-begin whole-state test now includes prepared authorization. A compile-
time size assertion bounds the preparation payload to the completion payload
already present in the phase enum; this adds no heap allocation or storage format.
It is not a firmware stack-usage measurement.

This is production shared-core code, but neither runtime calls the new preparation
method yet. The crash-consistency finding remains open. Next, wire engine admission
and both runtime adapters to the boundary, resolve indeterminate storage outcomes,
and remove rollback triggered solely by post-commit delivery failure. Controller-
side target access and grant management remain required parts of that integration.
Completion retention still uses the existing deadline; post-deadline delivery
retry behavior must be addressed explicitly rather than assumed from preparation.

Verification on macOS arm64: all 28 target-pairing tests, the root default-member
tests (`cargo test --locked`), core all-target clippy, registry checks and 49
website tests passed. The no-default-features `thumbv7em-none-eabihf` core check
also passed, including the payload-size assertion. All Cargo commands used
`CARGO_INCREMENTAL=0`. Filtered library tests named `remote_control_pairing`
passed through the separate Embassy (13 tests) and Tokio (3 tests) manifests.
These exercise existing callers, not adoption of preparation by either adapter.
No runtime crash-gap closure, full workspace or firmware
matrix, hardware, Kani or Miri result is claimed by this checkpoint.
