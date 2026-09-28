# Queued Embassy persistence through power cuts

The next recovery boundary executes the real Embassy persistence mailbox and
owner, not direct journal writes. Tests live beside that private owner so the
simulator does not require a new public maintenance API. Existing owner tests
were moved into their own module; production implementation text is unchanged.

One campaign admits a group replacement with room for an append. The second
begins with a full journal and requires the owner to persist its compaction
budget, erase the inactive arena, copy the confirmed state, commit that generation
and append the replacement. Each turn uses the real bounded persistence step.
Admission must initially be pending; settlement must occur within 32 turns.

The successful run supplies an operation manifest. Fault trials cut every byte
prefix of every write and erase, plus both boundaries of every read. There are
65 append cuts and 1,291 compaction/write cuts. The trace is capped at 128 entries;
no entries may be silently dropped. Each interrupted run must match the reference
prefix exactly. Independent model assertions check torn byte images, partial
reads and sticky power loss using the existing owner's test flash underneath.

Every interrupted request must settle as `Flash` failure without publishing the
candidate. A fresh owner, engine and exchange then restore the surviving byte
image twice. Recovery must yield the complete confirmed snapshot before the final
record commit word completes, and the complete candidate afterward. Lost
acknowledgement can therefore mean a failed request but committed durable state;
failure is not treated as proof that the write did not happen.

## Scope

Production behavior: unchanged. This is test coverage and module organization.
The fixture models contiguous prefix tears with immediate I/O. It lets the owner
observe the sticky flash error and settle before discarding volatile state; it
does not model scheduler cancellation during pending I/O or instruction-level
power removal. Reboots retain bytes only, not owner state or journal cursors.

This proves queued group persistence and compaction recovery. It does not prove
radio activation/rollback, controller-grant transactions, Tokio storage, a whole
running-node reboot, physical flash physics or target ISA execution.

## Verification

The registered `embedded-persistence-recovery` suite runs all owner tests on PR,
release and scheduled ladders, including these fault campaigns. The separate
`virtual-device-simulation` suite retains the radio and lower-level journal
campaigns. Miri source ownership follows the extracted directory; this slice
does not claim a new Miri or firmware qualification run.

```console
CARGO_INCREMENTAL=0 python3 validation/run.py run --suite embedded-persistence-recovery
CARGO_INCREMENTAL=0 cargo test --locked --manifest-path prns-runtime/impls/embassy/Cargo.toml --lib
CARGO_INCREMENTAL=0 cargo clippy --locked --manifest-path prns-runtime/impls/embassy/Cargo.toml --all-targets -- -D warnings
```

On macOS arm64, all 144 Embassy library tests passed, including all 32 tests in
the registered recovery suite (3.651 seconds). The registered simulation suite
passed in 89.013 seconds including compilation/build-lock waiting. Workspace
tests, Embassy clippy, format/docs, registry validation, website tests and all
16 assurance-selection tests passed. These durations are verification run times,
not production performance measurements.
