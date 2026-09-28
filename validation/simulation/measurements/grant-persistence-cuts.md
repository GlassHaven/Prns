# Controller-grant persistence across abrupt power removal

The owner-local NOR campaign now exercises production controller-grant snapshot
storage and restoration. A confirmed table contains an administrator and an
operator. One replacement changes only the operator's allowed request; another
removes the operator while retaining the administrator. Expected tables are
explicit typed grants, compared in canonical identity order after every reboot.

Each replacement runs both with append space and with a full arena requiring
compaction. The uninterrupted production store supplies the bounded operation
trace. Every write/erase byte prefix and both boundaries of every read become
an abrupt-removal trial. I/O stays pending at the selected boundary; the harness
drops the write future and reconstructs fresh owners from surviving bytes only.

| Replacement | Append cuts | Compaction plus append cuts |
| --- | ---: | ---: |
| Operator permission change | 197 | 1,555 |
| Operator revocation | 133 | 1,491 |

All 3,376 trials compare the exact trace prefix and restore twice. Before the
complete final record commit word, recovery must yield the full confirmed table.
Afterward it must yield the full replacement, even when the store never received
acknowledgement. Authority, identities and request sets are compared together;
no partial grant table, lost administrator or resurrected committed revocation
is accepted. Restore counts must also match, with no refused or dropped entries.

The owner's cached durable snapshot must remain confirmed while an interrupted
write is pending and adopt the candidate only after successful storage. The live
authorization table stays unchanged throughout the store call, including success:
activation belongs to the caller's transaction, not this persistence API.

## Scope and execution

Production impact: none. The tests reuse the owner-local bounded flash fault
model and real grant codecs, persistence owner and authorization restore path.
They do not exercise pairing transcripts, grant-management admission, live
activation/rollback, target-access snapshots, physical flash or a whole-node boot.
Fixture public keys are identifiers for storage tests, not signature evidence.

These cases run in the registered `embedded-persistence-recovery` suite alongside
the group campaigns. Use `CARGO_INCREMENTAL=0`:

```console
python3 validation/run.py run --suite embedded-persistence-recovery
cargo test --locked --manifest-path prns-runtime/impls/embassy/Cargo.toml --lib
cargo clippy --locked --manifest-path prns-runtime/impls/embassy/Cargo.toml --all-targets -- -D warnings
```

On macOS arm64, the registered recovery suite passed all 34 tests and the full
Embassy library passed all 146 tests. All-target Embassy clippy, registry and
website checks passed. The radio simulation, full workspace, Miri and firmware
matrix were not rerun for this test-only extension.

## Pairing rollback with the real persistence owner

The transaction follow-up connects `RemoteControlPairingPersistenceProgress`,
the authorization store exchange, its manifold adapter and the real flash owner.
It begins with a durable existing operator grant and prepares a permission change.
Preparation and a failed initial flash write must leave the complete live table
unchanged. The test verifies the exact failure-settlement command; its engine
acknowledgement is scripted, rather than executing a pairing link lifecycle.

Rollback then uses the real queue and journal. The first rollback write also fails.
The pending request must not complete before durability recovers, and no write may
consume the armed fault before the original retry deadline. A second retry deadline
must likewise be honored. Successful rollback returns the original typed initial
storage error, releases the progress state, and preserves prior live authority.

Journal inspection requires two complete copies of the confirmed snapshot: the
original and the newly persisted rollback. This prevents a false pass from merely
leaving the original record untouched. Two fresh restore owners must recover the
prior table. This is transient flash-failure coverage for the pairing transaction,
not abrupt power removal during rollback, remote grant-management admission,
successful pairing activation, or an end-to-end radio handshake.

Production impact remains none; the test now connects previously separate
transaction and storage assurances without changing their implementation.

The follow-up passed the registered recovery suite (35 tests), all 147 Embassy
library tests, all-target Embassy clippy, registry and website checks on macOS
arm64. No radio-simulation, full-workspace, Miri or firmware run is claimed for
this owner-local test extension.
