# Simulator roadmap and replay inputs

Current checkpoint: replay foundations, after the deadline/cancellation matrices.
This is a current-status guide, not another chronological list of test slices.

## Established foundation

- Real production nodes run against bounded frame and BLE models.
- Manual time coordinates medium effects and explicitly polled actors; seeded
  schedules select reproducible alternative actor orders.
- A two-node frame-only announce/link/echo scenario repeats complete medium
  traces, including packet bytes and shutdown, using explicitly supplied host
  entropy. This is not yet general BLE, restart or path-discovery replay.
- Correctness scenarios cover 128 frame nodes, 20 routed nodes and 16 BLE nodes.
  Backend-scale tests are not evidence for thousands of production nodes.
- Tokio/Embassy scenarios exercise real requests, Resource transfers, failures
  and recovery. Simulator findings have driven shared-core fixes.
- Request cancellation and overlapping deadline behavior have substantial
  regression coverage. Further permutations require a concrete new question.

## Input audit

| Input | Owner and current behavior | Replay implication |
| --- | --- | --- |
| Logical boot epoch | Tokio node lifecycle normally selects persistence or wall time; explicit-host construction carries a supplied timeline | Manual-fleet fixtures supply the host at construction, with a fixed epoch plus runtime elapsed time |
| Monotonic time | `ManualTimeDriver` owns paused Tokio time and validates medium coordination | Observe Tokio clocks inside runner-polled actors; outside-runtime reads use host time |
| Engine/inline-crypto entropy | `TokioHost<S>` owns shared-core `RuntimeEntropy<S>`; the default source is the OS | Explicit-host node constructors preserve the supplied source through real engine execution |
| Handle/interface entropy | `TokioEntropy` now owns an eagerly OS-seeded, node-scoped shared stream; handle clones, fleets and interface seams retain that owner | Executor-thread consumption no longer couples unrelated nodes; deterministic source selection is still absent |
| Path-discovery identifiers | `PrnsNodeHandle::request_path` uses a fallible `OsEntropySource` read through a private source seam | Failure-before-admission is tested; full-node replay still needs source ownership/selection for this path |
| Boot identities | Manual-fleet destinations and transport identities use explicit fixture secrets | Stable in this fixture; not a promise for arbitrary provisioning paths |
| Actor order | Manual task runner supports cyclic and versioned seeded scheduling | Does not determine branch selection inside futures or background-worker completion |
| Container ordering | Node handles include standard `HashMap` inventories | Audit iteration consumers before treating larger multi-interface ordering as reproducible |
| Process-command timestamps | Tokio process-command support reads `SystemTime` | Outside the current no-process scenario; do not call the entire runtime clock-controlled |

Source owners: Tokio `manifold/driver/host/mod.rs`, `runtime/entropy/mod.rs`,
`runtime/entropy/shared.rs`,
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

1. Finish explicit per-node entropy design across the already-owned host stream,
   the now node-scoped handle/interface owner, and path-request IDs, including
   source selection, restarts and reseeding. Keep OS entropy as the production
   default; avoid a process-global seed switch or weak shipping RNG mode.
2. Extend the bounded frame-echo packet replay to restarts and reseeding, then
   other transports once their exercised inputs are controlled. Retain
   changed-input controls; define a versioned replay artifact before exporting
   a stable format (current transcripts are private test values).
3. Expose/coordinate runtime deadlines before claiming arbitrary time jumps or
   long-duration acceleration. Bring worker completions under explicit control.
4. Measure full-node memory, active-peer cost and event throughput while scaling
   sparse routed and BLE fleets. Retain correctness and cleanup assertions.
5. Add Wi-Fi, persistence/power-loss and sleep models, then connect selected
   workloads to ISA emulators. Native radio/controller behavior and RF remain
   separate evidence; board names on virtual protocol profiles do not cover it.

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
