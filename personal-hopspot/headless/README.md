# Headless Personal Hopspot

A small host entry point for the shared Hopspot destinations and NomadNet pages.
It uses the Tokio runtime with a current-thread executor, a Reticulum TCP server,
and recipe-managed identity/route/ratchet persistence. The listen address and
private state directory must be supplied explicitly.

This first host supports TCP only. It does not configure radios, expose HTTP or
remote administration, enable Wi-Fi Auto, or establish HaLow broadcast support.

## Build and run

From the repository root:

```sh
cargo build --locked --release --manifest-path personal-hopspot/headless/Cargo.toml
personal-hopspot/headless/target/release/personal-hopspot-headless \
  --state-dir ./hopspot-state --listen 127.0.0.1:4242
```

Choose an interface address reachable by your peers for a network-facing node.
The port carries Reticulum over HDLC-framed TCP, not a web interface. The
`hopspot_ready` line reports the bound address and node-page destination.
Clients can request a path to that destination; the host does not periodically
announce to the network. Only attach it to networks you intend to participate in.

The state directory contains secret identity and vault material. On Unix the
directory is restricted to its owner. A process-held file lock prevents two
hosts from using the same state concurrently; the lock file can remain after
exit. Corrupt identity files cause startup to fail rather than silently replacing
the identity. The runtime batches retained-state writes, saves critical state,
and flushes on graceful shutdown. SIGINT and Unix SIGTERM request that shutdown.
`--run-for SECONDS` provides a positive-duration, bounded run.

Use a dedicated state directory. State in `/tmp` survives a process restart but
is lost at reboot on OpenWrt. Production installation must deliberately choose
persistent storage, available space, flash-write policy, and a service account.

## Verify a running host

Build and run the companion probe from a separate machine, substituting the
host's advertised destination hash:

```sh
cargo run --locked --manifest-path personal-hopspot/headless/Cargo.toml \
  --example fetch_page -- --target 192.168.12.1:4242 \
  --destination "$HOPSPOT_DESTINATION"
```

Within a 60-second deadline it connects, requests the destination's path,
establishes a Reticulum link, requests the index, and compares the complete
MessagePack response with the shared Hopspot page. Use matching source versions
on both ends; a changed page should fail this exact-content check.

```sh
cargo test --locked --manifest-path personal-hopspot/headless/Cargo.toml
cargo clippy --locked --manifest-path personal-hopspot/headless/Cargo.toml --all-targets -- -D warnings
```

Lifecycle tests exercise real localhost sockets, clean shutdown, identity
retention, rejection of concurrent writers, and preservation of a damaged
identity file. They require permission to bind localhost sockets.

## ThinkNode G4

See the [bring-up and installer procedure](docs/thinknode-g4.md) for the tested
cross-build, hardware evidence, temporary deployment, and remaining requirements
before offering persistent installation or firmware flashing to other users.
