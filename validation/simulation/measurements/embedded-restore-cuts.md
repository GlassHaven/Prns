# Embedded persistence restore across torn updates

The journal power-loss fixture now passes surviving flash images through
`EmbeddedFlashPersistence::restore`, using fresh production engine and Remote
Control assembly state for every boot. Snapshot decoding, logical-time recovery,
restore diagnostics and publication of interface group settings belong to the
production Embassy persistence owner, not a simulator reimplementation.

## Evidence

A valid baseline stores independent group sets for two stable interface IDs and
a persisted timebase. An update changes the first interface from one group to
two while preserving the second interface's distinct group. All 89 write-prefix
and verification-read boundary cuts through that append are exercised.

Each cut reconstructs the owner twice from the identical surviving byte image.
Before the final commit word completes, both boots must publish the complete
confirmed configuration; afterward, both must publish the complete candidate.
The independent oracle compares the entire restore report and typed snapshot,
including zero unrelated restore counters and the exact persisted clock floor.
Diagnostics must contain exactly the corresponding restore report, and the
owner must not report unsaved state.

The same sequential campaign then appends a malformed group snapshot inside a
valid committed journal record. The owner must refuse that payload, count the
refusal, and retain the previous valid configuration. Finally, a boot from blank
flash must publish an empty configuration and use raw boot time, rather than
retain the prior device image's process-global published groups or clock floor.

## Scope

Production impact: none. No production code, public API, dependency or firmware
memory changes are needed.

The writes under fault still use the production journal API directly. This
does not exercise the persistence owner's queued write transaction, command
activation/rollback, radio discovery startup, grant restoration or a complete
running node reboot. It also is not evidence for Tokio's separate store path.

The embedded restored-group exchange is process-global. All owner boots in this
integration executable intentionally run in one sequential test, and each owner
is dropped before the next boot. This proves repeated-image isolation, not
concurrent multi-node persistence ownership; future many-node integration must
provide an explicit owner/isolation seam rather than share this singleton.

The original bounded NOR model and its limitations still apply: contiguous
byte-prefix tears, immediate I/O, no flash physics or ISA emulation.

## Verification

Cargo commands use `CARGO_INCREMENTAL=0` on macOS arm64:

```console
cargo test --locked -p prns-simulation --test journal_power_loss -- --nocapture
cargo clippy --locked -p prns-simulation --all-targets --features controlled-time,heap-profile -- -D warnings
python3 validation/run.py run --suite virtual-device-simulation
cargo test --workspace --locked --quiet
bash validation/hygiene/fmt-docs.sh
./tools/prns verify
python3 validation/run.py verify
cargo test --locked --manifest-path docs/website/Cargo.toml --quiet
git diff --check
```

All five journal/restore tests passed, retaining the original 694 compaction cuts
alongside the 89 new owner-restore cuts. The registered simulation suite passed
in 62.535 seconds. Workspace tests, clippy, format/docs, registries and website
tests passed.
