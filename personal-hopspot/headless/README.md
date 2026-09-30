# Headless Personal Hopspot

A small host entry point for the shared Hopspot destinations and NomadNet pages.
It uses the Tokio runtime with a current-thread executor, a Reticulum TCP server,
and recipe-managed identity/route/ratchet persistence. The listen address and
private state directory must be supplied explicitly.

TCP is always available. The optional `wifi-halow` feature attaches a native
HaLoW interface on Linux; it does not configure the radio. HTTP, remote
administration, and Wi-Fi Auto are not exposed by this entry point yet.

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
Clients can request a path to that destination; the TCP-only host does not periodically
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

## Experimental HaLoW attachment

Build with `--features wifi-halow` (the G4 build task includes it). On an already
configured HaLoW device:

```sh
./personal-hopspot-headless --state-dir /tmp/hopspot-halow-state \
  --listen 127.0.0.1:4343 --halow-device wlan0 --halow-scope primary-halow \
  --halow-peers 16 --halow-announce-seconds 300
```

This requires Linux and `CAP_NET_RAW` (the vendor lab OS runs as root). Device and
scope must be supplied together. The scope is a stable, local 1–64-byte name;
preserve it across Linux interface renames. Omitting both retains TCP-only operation.
Socket binding fails startup before touching persistent state. The experimental
EtherType is `0x88b6` with the versioned Prns HaLoW envelope; both endpoints must
use this implementation. No radio profile or vendor service is modified.

Startup and periodic announcements advertise the node-page and delivery destinations
only on the radio's shared broadcast channel. The default interval is five minutes;
missed intervals are skipped. Idle peers expire after three intervals, checked every
five seconds. The peer cap defaults to 16. Each peer additionally owns bounded
runtime queues, so raise this cap only with a measured memory budget. Pacing estimates
are 7.3 Mbps unicast and 4 Mbps broadcast for the measured MCS2 profile. These are
payload estimates, not commands that set MCS or transmit power.

The `fetch_page` probe can use the radio instead of TCP:

```sh
./fetch_page --halow-device wlan0 --halow-scope probe-halow \
  --destination "$HOPSPOT_DESTINATION"
```

It waits for a radio peer, requests a path, establishes a link, and verifies the
whole page within 60 seconds. For bounded lab tests, set the server's announce
interval to 3 seconds and `--run-for 90`; otherwise its normal five-minute interval
can exceed the probe deadline. A G4 development bundle can include the native
probe with `./tools/prns run build.hopspot.g4 -- ... --with-probe`.

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

The [web installation guide](docs/g4-installation.md) is also embedded in the
website's `/flash/thinknode-g4` page. The
[HaLoW integration contract](docs/halow-integration.md) records the proposed
transport semantics, radio default, additional interfaces, and qualification work.

See the [bring-up and installer procedure](docs/thinknode-g4.md) for the tested
cross-build, hardware evidence, temporary deployment, and remaining requirements
before offering persistent installation or firmware flashing to other users.
