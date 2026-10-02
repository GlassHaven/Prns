# HaLoW software-transport qualification

This qualification exclusively targets the HaLoW software path before the next
G4/Heltec desk session. Four production Tokio nodes use the production `HaLow`
supervisor, envelope parser, source-MAC peer identities, broadcast and unicast
pacing, crypto, routing, Resource transfer and Remote Control. Application state
is `()`. Controller identities and target keys are explicitly provisioned in the
fixture; announcements are explicit actions. No physical devices are accessed.

## Medium and scheduling

`src/halow` owns a source/destination-aware datagram medium. Broadcast is one
accepted transmission delivered to reachable neighbors; unicast selects one MAC
and never falls back to broadcast. Directed paths permit asymmetric connectivity.
Kernel acceptance and receiver delivery remain distinct. `VirtualHaLowRadio`
implements the production cancel-safe datagram trait. Stalled or failed sends,
fatal receive errors, first-frame reception and queue pressure reach the actual
supervisor and peer tasks.

The existing manual driver coordinates medium delivery and Tokio timers.
Seeded cyclic actor ordering, fixed validation-only entropy streams and a fixed
logical boot epoch make complete traces comparable across fresh fixtures. Crypto
executes inline. A new clock variant identifies HaLoW advancement errors by
their owner. No native worker/thread interleaving or firmware timing is modeled.

Every attachment gets a monotonic radio ID. Pending delivery captures the
recipient generation, so a delayed packet cannot reach a replacement using the
same MAC. Reachability is sampled at transmission; already scheduled packets
remain in flight. Delay, loss and duplicate rules apply to explicit next matching
datagrams. Protocol workloads independently parse the struck packet and verify
its actual request, response, advertisement or Resource-part boundary.

| Owner | Explicit budget |
| --- | ---: |
| Live medium radios | 4 |
| Receive datagrams per radio | 256 |
| Pending medium deliveries | 512 |
| Armed faults | 16 |
| Retained medium events | 65,536 |
| Registered actors | 24 |
| Actor polls per settlement | 32,768 |
| App invocations | 256 |
| Announce observations | 4,096 |
| Production peers per supervisor | 16 |
| Production queued datagrams per peer | 16 |
| Production peer idle interval | 10 seconds; checked every 5 seconds |
| Control response deadline | 100 ms |
| Dense Resource response | 4,096 bytes; 30-second fault recovery budget |

Medium queue/pending capacity losses are recorded and fail full-node qualification.
The intentional pressure case fills the separate production peer lane. Its
128 one-byte invalid RNS frames have valid HaLoW envelopes: the test starts a
healthy controller request while they are queued and measures actual peer RX-byte
growth at a settled boundary. In the final routine profiles, 70 frames reached
that lane, with the remaining 58 dropped before delivery to it; the healthy
request and subsequent flooded-peer traffic passed. It does not invent a driver
drop counter. Trace overflow
retains the bounded prefix and reports omitted events; a qualified run requires
complete retention. Teardown remains usable even after trace-budget exhaustion.

## Contracts exercised

- Shared-medium discovery and a forced chain run real encrypted links and exact
  multi-frame Resource responses. Immediate peer identity comes from the relay
  MAC and local scope, rather than the remote destination. Removing the relay
  prevents learned routes from bypassing it; restoring it recovers traffic.
- An established asymmetric path loses requests while an independent controller
  continues. Restoring the missing direction recovers the original link.
- Local and ordinary relayed announces use broadcast. Non-announce packets remain
  unicast. Broadcast duplication and previous-hop echoes terminate within the
  production announce retry/jitter bounds, without adapter deduplication or
  automatic announcements.
- Control requests and responses tolerate delay/duplication; loss and delivery
  beyond the request deadline produce typed local timeouts. A verified late
  request can still execute. Late replies cannot settle another waiter. Concurrent
  traffic retains the correct controller identity and response ownership.
- Lost Resource advertisements and data parts recover. Later parts overtake a
  delayed first part, a subsequent part is duplicated, and the real receiver still
  returns all 4,096 expected bytes. Another controller progresses concurrently.
- Canceling a local caller does not undo app admission. Its delayed orphan reply
  cannot capture a replacement waiter on the same link.
- Stalled/failed sends preserve destination and allow independent-neighbor
  progress. The actual two-second send deadline releases stalled transport work.
  Idle expiry/re-admission preserves the MAC-derived interface ID.
- Delayed traffic for a retired adapter cannot complete on its replacement.
  Fatal receive failure detaches the supervisor and children. Explicit adapter
  replacement and a fresh node recover after observed peer-lane readiness.
- Real Remote Control build/interface/config/peer inspection and maximum-size
  96-byte app messages run over HaLoW alongside Resources. At the production
  16-peer cap, all peer rows and the shared broadcast child page in ID order;
  unavailable radio/rate measurements remain unavailable. Unlisted controllers
  receive no Response packet and never reach the app handler.

Transport MACs are not authorization credentials. A valid envelope from an
untrusted source can consume a bounded peer slot before Reticulum authentication.
The cap rejects additional sources without evicting existing peers. This is the
current admission contract, not a claim of Sybil resistance.

## Replay and resource evidence

Version 1 cases specify seed, topology and one atomic qualification scenario.
Unknown fields, versions and unsupported scenario/topology combinations are
rejected. Each scenario owns its prerequisites and recovery; there is no generic
application policy or production development registry.

Routine inputs use seeds `0`, `1`, `42`, `0x5eed`: 96 cases, each run twice.
Extended inputs use seeds `0..31`: 768 cases, each run twice. Comparison includes
every actual datagram byte, source/destination, scheduled copy, delivery outcome,
radio generation, app invocation, announce observation, semantic operation
counter and pressure measurement. Wall-time measurements are excluded.

Every case retains structured JSON for both fresh runs. Failed invariants and
replay mismatches retain their input and complete available trace; incomplete
retention is marked explicitly. Cases are atomic probes, so this slice does not
add a sequence reducer. Artifact roots are
`validation-artifacts/results/halow-simulation/{routine,extended,replay}/run-NNNN/`.
Extract an artifact's `case` object for standalone replay:

```console
./tools/prns repo.simulation.halow.replay --case validation/simulation/cases/halow/chain.json
python3 validation/run.py run --suite halow-simulation
python3 validation/run.py run --suite halow-simulation-extended
python3 validation/run.py run --suite halow-resource-stability
```

Runtime Resource rows, pending admission and any crypto ownership must drain
before shutdown. Shutdown requires all actors and medium radios, receive queues,
pending deliveries and armed faults to release. A separate isolated heap process
runs 32 construction/work/pressure/replacement/restart/drop cycles; its measured
retention budget is 64 KiB and peak budget is 32 MiB. These are harness bounds,
not a shipping-device footprint.

The initial chain broadcast probe caught a fixture horizon 24 ms short of the
last permitted retry; its boundary now derives from the production grace/jitter
constants. Reconnection probes wait for actual source-peer RX progress, since
announce pacing can defer transmission and core deduplication may suppress a
repeated announce event. Quiet-peer expiry requires disabling successful TX as
well as incoming traffic: active-link keepalives otherwise correctly refresh it.
These were fixture assumptions; no shipping protocol behavior was changed.

## Remaining hardware boundary

The medium models Ethernet datagrams around the configured radio. It does not
model modulation, sensitivity, airtime, collisions, hidden terminals, Linux
AF_PACKET or bridge behavior, Morse aggregation/retry behavior, RF range or
performance on the MIPS CPU. The existing board captures cover those adapter
boundaries separately, with their recorded limits.

Packet-socket rebinding remains explicit; this work does not add automatic
recovery. Persistent installation, update rollback, power loss, region-specific
radio configuration and long-running device resource limits still need desk
qualification. A software chain does not establish a forced physical RF chain.

## Verification

Final results on macOS arm64:

| Check | Result |
| --- | --- |
| `cargo test --locked -p prns-simulation --features controlled-time --lib halow` | 7 owner tests passed |
| Registered `halow-simulation` | 12 tests passed; 96 routine cases passed twice |
| Registered `halow-simulation-extended` | 768 cases passed twice |
| Registered `halow-resource-stability` | 32 cycles passed; peak 4,528,256 bytes; retained 56 bytes |
| Registered `virtual-device-simulation` | 447 passed, 13 explicitly ignored; 178.539 seconds |
| Tokio interface `--features wifi-halow --lib wifi_halow` | Both existing adapter tests passed |
| Headless `--features wifi-halow --test halow_multihop` | Existing two-hop test passed |
| Simulator Clippy `--features controlled-time,heap-profile --all-targets -- -D warnings` | Passed |
| Ordinary `cargo check --locked -p prns-simulation` | Passed |
| Named HaLoW standalone replay task | Chain case passed twice |
| Registry verification, formatting and diff checks | Passed |

Final routine artifacts are `routine/run-0006` (96 files), expanded artifacts
are `extended/run-0002` (768 files), and standalone chain replay is
`replay/run-0001`. Every campaign file contains both traces and public
observations. Earlier runs and initial failing fixture evidence remain separate.
The new focused suites also run under the normal verification registry.

`./tools/prns verify` still fails on the pre-existing placement of
`personal-hopspot/headless/scripts/network-lab/{lease.sh,prepare.py,radio.sh}`.
Those unrelated scripts were not moved. The replay task itself works. No shipping
mutation-analysis surface changed, so no production mutation run was required.
Other operating systems, Linux packet sockets, target ISA execution and physical
devices were not run.
