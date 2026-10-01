# Remote Control durable authority qualification

The implementation follows [the roadmap](../remote-control-roadmap.md). The baseline packet campaign remains in [Remote Control qualification](remote-control.md).

## Controlled I/O foundation

Tokio persistence now accepts an explicit `PersistenceIo` driver. Native construction continues to execute filesystem jobs on the blocking pool. The recipe-managed worker and authorization transactions use the same driver; flush revisions and authorization ownership still belong to their existing shared owner. Execution labels distinguish authorization begin/store/confirm/finish, ordinary flush revision/commit and vault work. The driver does not decide grants, activation, rollback or retry timing.

The four-node fixture adds a separately granted operator beside the administrator and unlisted controller. Each scenario owns isolated real `FileStore` storage. The controlled driver executes I/O within the manual actor, can gate selected operation completion and supplies typed scripted write/confirmation outcomes. Node reconstruction preserves fixture keys and changes boot entropy explicitly. Initial authority is provisioned with the existing snapshot/flush API before the scenario begins.

Three whole-node cases cover gated writes followed by two restores, published candidates with failed/missing/different confirmation followed by recovery, canceled callers after publication, and failed candidate writes followed by durable rollback. Whole typed grant tables, packet transcripts and I/O observations are compared. The published-unconfirmed case deliberately writes a real durable file then withholds confirmation; it tests owner behavior under uncertainty, not a filesystem crash mechanism.

On macOS arm64 the 20-test Remote Control target passed; the native Tokio library passed 306 tests with one ignored. Focused native persistence tests passed 20. Strict Tokio library/test and simulator Remote Control clippy passed. Commands:

```console
cargo test --locked -p prns-simulation --features controlled-time --test remote_control --quiet
cargo test --locked --manifest-path prns-runtime/impls/tokio/Cargo.toml --lib persistence --quiet
cargo test --locked --manifest-path prns-runtime/impls/tokio/Cargo.toml --lib --quiet
cargo clippy --locked --manifest-path prns-runtime/impls/tokio/Cargo.toml --lib --tests -- -D warnings
cargo clippy --locked -p prns-simulation --features controlled-time --test remote_control -- -D warnings
```

The remaining roadmap milestones are not yet qualified. No hardware or other-platform result is implied.
