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
- `prns-runtime/impls/tokio/src/runtime/remote_control_pairing_persistence/mod.rs`
- `prns-runtime/impls/tokio/src/runtime/remote_control_controller_grants/mod.rs`

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

At this first checkpoint neither runtime called the new preparation
method yet. The crash-consistency finding remained open. The next step was to wire engine admission
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

## Rollout checkpoint: target grant admission and delivery

The shared engine now prepares completion before emitting
`RemoteControlTargetPairingAuthorizationRequired`, in both approval orders.
This is the production persistence event consumed by Tokio and Embassy: no
adapter-specific preparation policy is needed. Preparation failure aborts the
matching attempt and retires its exchange without requesting a candidate store.
The local approval result and ingress diagnostic report that failure explicitly.

After storage, the engine consumes its retained completion without fetching the
signer again. A real-engine regression checks slow persistence beyond the original
deadline with the signer removed after preparation: completion is dispatched,
not rejected for durable rollback. Another checks that removing the signer before
preparation requests no storage in either approval order.

Both runtimes now preserve committed authority when settlement reports completion
dispatch failure or completion-retention expiry. The classification is shared
core policy; the adapters report an error without undoing the grant. Tokio retains
a typed `CommittedCompletionDelivery` error. Embassy releases transaction ownership
while returning its typed settlement error. Completion delivery has not become a
success merely because authorization committed.

The Embassy fixture drives the real queue, activation and flash owner through both
delivery failures, checks the exact prior/candidate record history without a
rollback record, and restores the complete candidate table on two fresh boots.
Its settlement acknowledgement is still scripted. A Tokio finalization-unit test
checks the same live-authority rule; it is not host filesystem durability evidence.

The existing 133-cut rollback characterization deliberately injects the old
`AuthorizationRollbackRequired` acknowledgement. It remains useful evidence for
the rollback fallback, but is no longer evidence that the prepared production
engine requests rollback merely because storage crossed the admission deadline.

Still open: indeterminate writes and missing settlement acknowledgements, activation
inconsistency after commit, controller-side target-access persistence, grant-
management cancellation, post-deadline completion retry semantics, and whole-node
power-loss coverage. No new persisted format is introduced. Full transaction
crash-consistency closure is not claimed.

On macOS arm64, root default-member tests, all 272 active Tokio library tests
(one ignored), all 151 Embassy library tests, 218 focused core Remote Control
tests, and the registered 39-test embedded persistence recovery suite passed.
Core and both runtimes passed all-target clippy; the no-default-features ARM core
check passed. Cargo runs used `CARGO_INCREMENTAL=0`. Firmware/resource builds,
physical devices, full workspace, Miri and Kani were not run for this slice.

## Rollout checkpoint: grant-management response loss

Tokio and Embassy no longer undo a successfully stored and activated controller
grant change because a local caller cancelled or a remote response failed. This
covers grant creation, replacement and revocation. In particular, losing the
response to a committed revocation must not resurrect the controller.

Cancellation before command processing retains the existing no-mutation behavior.
Once storage succeeds, both adapters finish activation independently of the
completion receiver. The existing response failure handling remains in place;
neither path claims the peer received a result. Initial-store and activation
failures still use their existing recovery paths and remain subject to the open
indeterminate-write audit. No shared-core state or storage format changes here:
the removed rollback decisions belong to runtime delivery mechanisms.

The new Tokio test pauses the real storage worker with its existing mutex before
polling the application future. It proves the old snapshot remains readable while
the write is pending, cancels the local receiver, then releases the worker. Six
cases cover add/replace/revoke with either local cancellation or a missing remote
node. Each compares the complete live table and decodes the file through two new
store instances, preserving an unrelated administrator. The pause accessor is
test-only; it adds no shipping state or storage backend. No sleeps or scheduler
timing guesses determine the cancellation boundary.

The existing Tokio router test now injects an actual response settlement failure
and checks both live and stored authority. Embassy tests drop actual local request
futures during pending storage and fill the command queue to reject a remote
response. They verify the full table, ready state where directly available, and
absence of a newly queued rollback store. Embassy storage completion is scripted
in these router tests; the NOR owner campaign remains separate evidence. Host file
reopening is not filesystem power-loss or full-node boot evidence.

Verification on macOS arm64: all 273 active Tokio library tests (one ignored),
all 152 Embassy library tests, and all-target clippy for both runtimes passed.
The narrow filters were `remote_control_controller_grants` on Tokio (7 tests)
and `request_runner` on Embassy (11 tests). Cargo commands used
`CARGO_INCREMENTAL=0`. The full firmware/resource matrix, Miri, Kani and physical
hardware were not run for this adapter-only change.

Remaining production work: indeterminate storage and missing pairing settlement
acknowledgements, activation inconsistency after commit, controller-side target-
access transactions, and completion-retention/retry semantics. Grant-management
response-loss rollback is now fixed; full crash-consistency closure is not claimed.

## Rollout checkpoint: controller-side target-access settlement

Both adapters now retain successfully stored and activated target access when
the subsequent engine settlement fails. Tokio reports distinct typed errors for
a missing acknowledgement, rejected settlement, or inconsistent finalization.
Embassy releases transaction ownership and returns its existing typed settlement
error without scheduling a rollback. Neither adapter reports successful pairing
when settlement failed. This does not introduce a completion retry mechanism.

Shared controller pairing already preserves its prepared access in `Persisting`
through link closure and expiry; these rollback decisions belonged to the adapters.
Initial-store and activation failure recovery are unchanged and remain subject
to the indeterminate-write audit.

The Tokio test exercises the real file worker for add and replacement, preserving
an unrelated target. It checks committed contents before scripted acknowledgement,
the entire live table afterward, and two fresh file readers. Cases cover success,
lost acknowledgement, attempt mismatch, no persistence owed, and unexpected failure
finalization. These are not filesystem power-loss tests.

The Embassy test exercises the actual flash-journal owner for the same add/replace
cases. It checks success, attempt mismatch, no persistence owed, unexpected failure
finalization, and an occupied pairing-settlement slot. It verifies released
transaction ownership, no queued rollback, exactly the prior and candidate records,
and the complete candidate table on two fresh restores. Engine acknowledgements
are scripted; these restores do not claim whole-node boot or hardware coverage.

Remaining work includes indeterminate storage, target-side missing settlement
acknowledgements, activation inconsistency after commit, completion-retention/retry
semantics, and whole-node crash coverage. Full crash-consistency closure is not
claimed.

Verification on macOS arm64, with `CARGO_INCREMENTAL=0` for Cargo runs:

- `cargo test --locked --manifest-path prns-runtime/impls/tokio/Cargo.toml --lib`:
  274 passed, one ignored; the focused `remote_control_pairing_persistence` filter
  passed all four tests.
- `cargo test --locked --manifest-path prns-runtime/impls/embassy/Cargo.toml --lib`:
  153 passed.
- `cargo clippy --locked --manifest-path <runtime>/Cargo.toml --all-targets -- -D warnings`:
  passed for both runtime paths above.
- `python3 validation/run.py run --suite embedded-persistence-recovery`:
  all 40 tests passed.
- `./tools/prns verify`, `python3 validation/run.py verify`, and
  `cargo test --locked --manifest-path docs/website/Cargo.toml`: passed.

No shared-core code changed. Root/workspace tests, firmware/resource builds,
physical hardware, Miri and Kani were not run for this adapter-only slice.

## Rollout checkpoint: unavailable target-grant settlement

Tokio now retains a successfully stored and activated controller grant when the
settlement channel closes without an acknowledgement, returning
`CommittedTargetGrantSettlementUnavailable`. Embassy likewise retains the grant
when its pairing-settlement slot is busy or its settlement helper reports
`NodeStopped`, releasing transaction ownership and returning the typed failure.
An unavailable acknowledgement does not establish that committed authority was
rejected. Explicit engine-directed rollback and explicit core rejection retain
their existing recovery behavior; those are distinct from this transport failure.

The regression tests failed before the production changes. Tokio removed the
committed live grant; Embassy entered rollback rather than returning the busy
settlement failure. The Tokio fixture exercises the actual file writer for both
add and replacement, with either a closed command receiver or a dropped settlement
sender. It checks the candidate on disk before dropping an issued acknowledgement,
the complete live grant table afterward, and two fresh file reads.

Embassy extends the existing flash-owner fixture with an actually occupied
settlement slot and a mismatched typed acknowledgement, which the node handle
maps to `NodeStopped`. The latter is a scripted adapter failure, not a simulated
physical node shutdown. Both cases check the precise error, released ownership,
no queued rollback, exactly the prior and candidate journal records, and the
complete candidate grant on two fresh restores. Existing successful settlement,
delivery failure, expiry, explicit rollback and interrupted-rollback cases remain
in the same fixture.

No shared-core state or persisted format changes. Indeterminate writes, activation
inconsistency, explicit target settlement rejection, completion retries and
whole-node crash coverage remain open; this is not full crash-consistency closure.

Verification on macOS arm64 with `CARGO_INCREMENTAL=0`: the focused Tokio
`remote_control_pairing_persistence` filter passed five tests; the focused Embassy
`committed_authority_survives_unavailable_settlement` regression passed. Full
`cargo test --locked --manifest-path <runtime>/Cargo.toml --lib` runs passed
275 Tokio tests (one ignored) and 154 Embassy tests. Both runtime paths passed
`cargo clippy --locked --manifest-path <runtime>/Cargo.toml --all-targets -- -D warnings`.
`python3 validation/run.py run --suite embedded-persistence-recovery` passed all
41 tests. Formatting, both registries, and website tests also passed. Root/workspace,
firmware/resource, hardware, Miri and Kani checks were not run for this adapter slice.

## Rollout checkpoint: stale target-attempt settlement

Both adapters now retain committed controller grants when target settlement
returns `NoAuthorizationOwed` or `AttemptMismatch`. Those outcomes describe the
engine's current attempt ownership, not a revocation request. Tokio preserves
the exact error in `CommittedTargetGrantSettlement`; Embassy releases transaction
ownership and returns its existing typed target-settlement error. Neither reports
successful pairing. Shared core already preserves an unrelated active attempt
on mismatch, so no core production change is needed.

Both regressions failed before the adapter changes: Tokio removed the live grant,
and Embassy entered rollback. The Tokio file-worker fixture now covers eight
add/replace cases across unavailable and stale settlement, checking the complete
table in memory and through fresh file readers. The Embassy flash-owner fixture
injects both stale-attempt errors and checks the returned error, ready/released
state, no rollback request, prior/candidate record order and complete candidate
table on two fresh restores. Settlement errors are scripted; this is not evidence
of a whole-node race or physical power loss.

Explicit `AuthorizationRollbackRequired`, signing failures, and inconsistent
finalizations retain their existing behavior and need separate audit. Indeterminate
writes, activation failure recovery, completion retries and whole-node crash
coverage also remain open. No persisted format changes or complete crash-consistency
claim accompanies this slice.

Verification on macOS arm64 with `CARGO_INCREMENTAL=0`:

- `cargo test --locked --manifest-path prns-runtime/impls/tokio/Cargo.toml --lib`:
  275 passed, one ignored; the focused pairing-persistence filter passed five tests.
- `cargo test --locked --manifest-path prns-runtime/impls/embassy/Cargo.toml --lib`:
  155 passed, including the focused stale-attempt regression.
- Both runtimes passed `cargo clippy --locked --manifest-path <runtime>/Cargo.toml --all-targets -- -D warnings`.
- `cargo test --locked -p prns-core authorization_settlement_preserves_mismatch_and_absence`:
  one existing shared-engine test passed.
- `python3 validation/run.py run --suite embedded-persistence-recovery`:
  42 tests passed.
- Formatting, `./tools/prns verify`, `python3 validation/run.py verify`, and
  `cargo test --locked --manifest-path docs/website/Cargo.toml` passed.

Root/workspace suites, firmware/resource builds, hardware, Miri and Kani were not
run for this adapter-only change.
