# Simulator roadmap and replay inputs

Current checkpoint: replay foundations, after the deadline/cancellation matrices.
This is a current-status guide, not another chronological list of test slices.

## Established foundation

- Real production nodes run against bounded frame and BLE models.
- Manual time coordinates medium effects and explicitly polled actors; seeded
  schedules select reproducible alternative actor orders.
- A two-node frame-only announce/link/echo scenario repeats complete medium
  traces, including packet bytes and shutdown, using explicitly supplied host
  entropy. That bounded scenario now also covers a receiver restart and the
  successful/failed host reseed boundary. This is not general BLE or
  path-discovery replay.
- A bounded path-discovery/echo variant now supplies all three runtime entropy
  providers and compares interface draws, handle draws, path IDs and packet
  traces. Changing either shared-stream or path-source input changes output.
- Eight real BLE nodes now replay four concurrent pairs, including selective
  reconnect while unaffected links continue traffic, under cyclic and three
  seeded actor orders. Whole wire/discovery traces repeat for each fixed input.
  This is paired-fleet replay, not multi-peer mesh or scale-performance evidence.
- A three-node BLE star repeats complete transcripts with two concurrent leaves
  sharing one supervisor. Disconnecting one leaf preserves the other's original
  working link; recovery replaces only the affected connection. This adds
  multi-peer ownership coverage, not routed-mesh or scale-performance evidence.
- Correctness scenarios cover 128 frame nodes, 20 routed nodes and 16 BLE nodes.
  Backend-scale tests are not evidence for thousands of production nodes.
- The paired BLE replay fixture now covers 32 nodes in ordinary tests and
  128 nodes in an opt-in lifecycle timing probe. The probe retains exact replay
  and cleanup assertions; its debug timings include tracing and are not peak
  memory or sustained network throughput measurements.
- An opt-in allocator probe now measures live/peak requested heap and allocation
  totals at 8/32/128 nodes. Right-sizing the paired fixture's trace reserve reduced
  its 128-node measured peak from 325 MB to 57 MB without trace eviction.
- Tokio wake-driven stepping now coordinates registered actors, timer wakes,
  medium events and a caller horizon without fixed tick polling. A day-long idle
  two-node scenario resumes real traffic, then stops at a real request deadline;
  repeated hourly timers verify rearming over a simulated day.
- Tokio/Embassy scenarios exercise real requests, Resource transfers, failures
  and recovery. Simulator findings have driven shared-core fixes.
- Embassy now has observable timer deadlines coordinated with the same runner,
  exact two-node BLE wire replay with changed-payload/entropy controls, and a
  process-isolated heap probe that accounts for static fixture storage separately.
  These are bounded Embassy results, not mixed-runtime byte replay or fleet scale.
- The mixed-runtime follow-up now repeats complete bidirectional BLE transcripts
  through partition/reconnect for an Embassy/Tokio pair. Two platform-profile
  pairings and ten initial readiness orders each replay their own fixed input.
  This closes that bounded mixed replay gap, not arbitrary topology or fleet scale.
- Request cancellation and overlapping deadline behavior have substantial
  regression coverage. Further permutations require a concrete new question.

## Input audit

| Input | Owner and current behavior | Replay implication |
| --- | --- | --- |
| Logical boot epoch | Tokio node lifecycle normally selects persistence or wall time; explicit-host construction carries a supplied timeline | Manual-fleet fixtures supply the host at construction, with a fixed epoch plus runtime elapsed time |
| Monotonic time | `ManualTimeDriver` owns paused Tokio time and validates medium coordination | Observe Tokio clocks inside runner-polled actors; outside-runtime reads use host time |
| Engine/inline-crypto entropy | `TokioHost<S>` owns shared-core `RuntimeEntropy<S>`; the default source is the OS | Explicit-host node constructors preserve the supplied source through real engine execution |
| Handle/interface entropy | `TokioHandleEntropy` owns a branded stream; handle clones, fleets and interface seams share ownership | Full-node construction now accepts an explicit provider; ordinary constructors remain OS-backed |
| Path-discovery identifiers | The same owner retains a separate fallible source; the public `request_path` API consumes it before admission | Explicit provider selection and failure-before-admission are tested, including successful path discovery in the simulator |
| Boot identities | Manual-fleet destinations and transport identities use explicit fixture secrets | Stable in this fixture; not a promise for arbitrary provisioning paths |
| Actor order | Manual task runner supports cyclic and versioned seeded scheduling | Does not determine branch selection inside futures or background-worker completion |
| Internal readiness | Node interface-driver and BLE supervisor selection are independently configurable; normal defaults remain Tokio-fair | Explicit per-instance rotation supports the two-node, paired-fleet and shared-hub replay fixtures; other selectors remain scenario-dependent |
| Container ordering | Node handles include standard `HashMap` inventories | Audit iteration consumers before treating larger multi-interface ordering as reproducible |
| Process-command timestamps | Tokio process-command support reads `SystemTime` | Outside the current no-process scenario; do not call the entire runtime clock-controlled |

Source owners: Tokio `manifold/driver/host/mod.rs`, `runtime/entropy/mod.rs`,
`runtime/entropy/shared/mod.rs`,
`runtime/node_facade/path_discovery/mod.rs`, `runtime/node_facade/node_lifecycle/mod.rs`,
`runtime/node_facade/persistence/mod.rs`, and `runtime/process_commands.rs`.
Shared entropy policy already belongs to core; do not duplicate its generator
or reseeding rules in the simulator.

## This three-slice round

1. Audit and roadmap: distinguish normalized scenario repeatability from packet
   replay, and inventory the uncontrolled inputs above.
2. Timeline seam: reuse `with_timeline_origin` in the shared manual-fleet boot
   helper. An epoch of 1,000,000 logical milliseconds plus checked runtime
   elapsed time keeps fresh boots and later restarts on one scenario timeline.
   The runner cannot advance while a newly admitted actor is ready, so the
   admission snapshot remains valid through its first boot poll. Production
   constructors initially sampled their normal inputs before this override.
   The later node-host round replaces that override with explicit construction,
   avoiding the wall-time sample in this helper.
3. Repeatability evidence: four real nodes, two isolated pairs, a real link and
   echo before/after one node restarts at seven milliseconds. Three fresh runs
   compare whole normalized transcripts. A changed equal-length application
   value must change observations. Existing fixture assertions verify teardown.

The transcript retains actual response values, boot/current logical clocks,
ordered medium events, endpoint IDs, transmission ordinals, times and frame
lengths. It deliberately excludes transmitted packet bytes, including random
link IDs, keys, signatures and ciphertext. It is captured before final fleet
shutdown; teardown is independently asserted by the fixture. This is a private,
bounded test projection, not a stable serialized replay format. Only the cyclic
actor schedule and this short scenario are claimed repeatable.

An initial clock assertion failed because the test read `TokioClock` outside
the runner. Clock observations now execute as runner actors and validate exact
expected values. No production clock change was needed.

## Next milestones, in order

### Embassy parity checkpoint

Embassy is a first-class simulation target, not deferred hardware-only work.
Existing mixed-runtime request/Resource, cancellation and recovery scenarios run
its real runtime and share protocol-core fixes. The initial complete-byte replay,
fleet heap measurements and automatic wake stepping were Tokio-only evidence.
The [Embassy follow-up](measurements/embassy-parity.md) now closes these specific
fixture gaps without treating Tokio results as Embassy evidence.

- Clock coordination: `embassy_ble/clock/` leases the process-global test clock
  and exposes the next deadline from Embassy's upstream bounded timer queue.
  Completion and mixed-runtime discovery loops coordinate it with Tokio and
  medium events. Alternating timers cover a simulated day; a real Embassy BLE
  supervisor stops at its ten-second greeting deadline with a day-long horizon.
- Replay: the existing `SharedRuntimeEntropy` fixture source now varies
  independently of address. Two Embassy nodes repeat complete wire/discovery
  transcripts with payload and entropy controls. The
  [mixed-runtime follow-up](measurements/mixed-runtime-replay.md) adds complete
  bidirectional transcripts through partition/reconnect. Full Embassy node restart
  replay and larger Embassy fleets remain unproven.
- Memory/scaling: production Embassy APIs require static channel, lane and
  entropy lifetimes. The fixture now counts their direct storage; the opt-in
  two-node heap probe uses a fresh child process so retained allocations from
  earlier tests cannot contaminate it. Static storage is deliberately retained
  until process exit, not unsafely reclaimed or described as a production leak.
  Larger Embassy fleets and repeated lifecycle memory behavior remain open.
- Shared ownership: retain the common medium, wire capture, actor scheduling,
  clock validation and protocol-core fixes. Each new milestone should identify
  evidence for both adapters, or name the specific unresolved adapter limitation.

These are coverage/design follow-ups, not evidence of an Embassy production bug.
Extend this adapter evidence alongside subsequent milestones rather than expanding
into another Tokio-only scenario family.

The reconnect extension exposed an additional input: internal readiness
arbitration. The [arbitration follow-up](measurements/ble-replay-arbitration.md)
now controls both the interface-task driver and BLE supervisor per instance,
restoring exact whole reconnect transcripts across ten initial-order combinations.
Production defaults remain Tokio-fair. This closes the observed two-node
reconnect gap, not every nested selector, worker or native backend schedule.
The [paired-fleet follow-up](measurements/ble-paired-fleet-replay.md) composes
those controls with outer seeded actor order across concurrent links.
The [shared-hub follow-up](measurements/ble-shared-hub-replay.md) exercises
concurrent connections owned by one supervisor without additional runtime controls.

1. Extend the three-provider construction seam beyond the now-proven short BLE
   replay to other backend consumers, auditing randomness outside node-owned providers. Keep
   OS entropy as the production default; no global seed switch or weak
   shipping RNG mode exists. Host restart/reseed evidence must not be treated
   as shared-source/backend lifecycle coverage without exercising those paths.
2. Carry the bounded frame/BLE echo replay approach to other transports once their
   exercised inputs are controlled. Receiver restart and successful/failed
   periodic reseeding now have focused packet evidence. Retain
   changed-input controls; define a versioned replay artifact before exporting
   a stable format (current transcripts are private test values).
3. Extend coordinated Tokio/Embassy wake-driven stepping to broader long-duration
   workloads. External worker completions remain outside both controlled timer
   queues and require separate ownership and evidence.
4. Measure full-node memory, active-peer cost and event throughput while scaling
   sparse routed and BLE fleets. Retain correctness and cleanup assertions.
   [Initial BLE lifecycle timings](measurements/ble-fleet-scaling.md) now cover
   8, 32 and 128 real nodes; allocation/RSS and phase-level attribution remain open.
   [Phase attribution](measurements/ble-fleet-phase-costs.md) subsequently exposed
   redundant all-radio schedule scans during actor polls. Clock validation now
   reads current ticks without searching for future events; peak-memory and
   finer runtime/crypto attribution remain open.
5. Add Wi-Fi, persistence/power-loss and sleep models, then connect selected
   workloads to ISA emulators. Native radio/controller behavior and RF remain
   separate evidence; board names on virtual protocol profiles do not cover it.

The [heap and deadline checkpoint](measurements/heap-and-deadlines.md) records
the bounded scale baseline, explicit heap-profile command, wake-stepping contract
and long-duration evidence. RSS and thousand-node capacity are not yet measured.

Production impact of the initial timeline round: none. The existing timeline API is reused in
tests; entropy remains OS-backed. Verification evidence is recorded in
[the replay foundation measurement](measurements/replay-foundation.md).

The [entropy ownership follow-up](measurements/node-entropy-ownership.md) changes
Tokio production ownership from thread-local to node-scoped for handles and
interfaces, retaining the core CSPRNG. Its synchronization and memory costs are
explicit; it does not yet seed or replay production packets deterministically.

The [host source seam follow-up](measurements/host-entropy-source.md) allows a
low-level Tokio host to consume a supplied core stream without adding a new
generator or global switch. Path discovery keeps its fallible source contract.
The [node-host follow-up](measurements/node-host-replay.md) carries that source
through real nodes and proves a bounded frame-only packet trace. Shared
handle/interface and path-ID source selection remain unfinished; their consumers
must be controlled before widening the replay claim.

The [restart/reseed follow-up](measurements/restart-reseed-replay.md) records
source reads by node and boot incarnation. It retains whole packet traces
through restart and drives the real core reseed policy at its byte boundary.
These tests add no shipping behavior or new entropy-source implementation.

The [owned-input follow-up](measurements/owned-entropy-inputs.md) completes source
selection for the three audited node-owned providers and exercises their public
consumers. It records the shared owner's dispatch/memory cost and the remaining
limits before claiming broader replay.

The [BLE wire replay follow-up](measurements/ble-wire-replay.md) compares every
accepted control value and GATT fragment across three fresh two-node runs, with
changed-seed and equal-length changed-payload controls. Capture is bounded and
opt-in; this original round is not native Bluetooth, RF, reconnection-incarnation,
or arbitrary scheduler replay evidence. The incarnation extension below records
the precise limit discovered by widening the scenario.
