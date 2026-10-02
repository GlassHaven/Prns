# Asynchronous and overlapping core qualification

This slice adds a non-shipping scheduling seam and qualifications for the real
core and both runtimes. It does not replace production crypto, persistence,
protocol execution or public outcomes with simulated responses.

## Scope and contracts

The three-node BLE star has a primary controller, a target and an independently
paired controller. Actual identities, permissions, encrypted links, application
requests, channels, Resources, FileStore and NOR journal implementations participate.
Application state is ordinary `()`. Announcements are explicit fixture actions.
Unsupported watch and measurement capabilities remain unavailable. Unauthorized
requests remain silent. Canceling a local waiter does not withdraw a request or
undo an admitted application operation.

There are ten profiles: four Tokio/Embassy controller-target pairings, with inline,
one controlled worker and four controlled workers in each Tokio-containing
pairing. Embassy-only uses inline execution. Both controllers use the selected
controller runtime. A separate native full-node loopback test runs real threads
with one and four workers; controlled scheduling does not claim to emulate thread
interleavings or native batch formation.

`simulation-control` is absent from the runtime's default features. Its worker
queues, admission, work accounting, result rings, crypto implementation, wakeup
and manifold consumption are the production owners. The harness selects actual
execution and publication transitions. Worker IDs, job IDs, boundaries, work
kinds, occupancy and retirement are typed and bounded. Feature-disabled builds
contain none of these scheduling hooks.

## Behavioral evidence

- Valid signature execution, independent publication, held results, interactive
  precedence, reversed four-worker completion and actual ring backpressure.
- Retirement before execution, after execution and after publication, with fresh
  pool ownership and old-generation rejection.
- Full-node resource builds held at execution and publication. A second controller
  completes while the primary is pending; releasing the actual result delivers
  the expected bytes. A 16 KiB Resource exercises worker `OpenSpan` publication.
- Closing a link or retiring the primary with a computed resource result cannot
  land that result on replacement links or nodes.
- Concurrent bounded Resources, ordinary requests, channels and authenticated app
  messages retain the correct controller/link/command ownership.
- Channel window pressure settles one `WindowFull` failure; received commands
  settle once. Public outcomes agree with actual Tokio outcome counter deltas.
- Response refusal retains `ResponseTooLarge` across transfer cancellation. The
  link and request capacity remain usable afterward. Lost responses become local
  timeouts; canceled callers do not undo the verified handler invocation.
- Inventory includes both peers, and absent rate measurements stay absent.
  Implemented watches deliver their initial resync and refetch after reconnect.
  Byte streams deliver complete bytes and EOF; duplicate readers are rejected.
- FileStore authority writes can be held while engine channel work continues.
  Fresh endpoint admission waits for the authority transaction, then completes
  after release. This is the current serialized authority boundary; it is not a
  promise of endpoint progress through an indefinitely stalled storage owner.

The common resource window is 1,024 bytes and the radio transmit depth is four.
The large-worker probe has a separately selected 32 KiB window. Budgets include
framing/encryption overhead. Native radio timing, modulation, HaLoW, CPU throughput
and deployment performance are outside this qualification.

## Replay and reduction

Version 1 cases specify seed, runtime/execution profile, family and atomic actions.
The routine matrix is four seeds × ten profiles × three families = 120 cases,
32 actions each. Extended inputs use seeds 0..63 and 64 actions = 1,920 cases.
Each case is process-isolated and compares full traces from two fresh fixtures.
Four bounded child workers use a pinned executable. Wire connection IDs are
normalized; bytes, persistence operations, events, crypto order and static
allocation evidence must match exactly within a profile. Wall-time crypto timing
is excluded from the semantic counter projection.

Passing artifacts keep inputs, observations and SHA-256 trace digests. Seed 42
reference cases and failures retain full traces. Runtime, invalid-input and replay
failures have distinct classes. The bounded reducer preserves the action and
original diagnostic in both fresh runs. Original evidence and every candidate
are retained in separate files. Standalone reduction first reproduces the original
and refuses to claim a repaired failure is still reproducible.

The first routine campaign reduced an invalid fixture sequence to two successive
byte-stream operations that reused a link-owned stream ID. The corpus now checks
reader ownership and closes the old link before starting the next stream. No
production behavior was changed to satisfy that mistaken assumption.

```console
python3 validation/run.py run --suite virtual-device-simulation
python3 validation/run.py run --suite core-work-simulation-extended
python3 validation/run.py run --suite core-work-resource-stability
./tools/prns repo.simulation.core-work.replay --case PATH/TO/case.json
./tools/prns repo.simulation.core-work.reduce --case PATH/TO/original.json
```

Campaign artifacts are under
`validation-artifacts/results/core-work-simulation/{routine,extended,replay,reduction}/run-NNNN/`.
Existing Remote Control formats, artifacts and tasks are unchanged.

## Resource qualification

A separate isolated heap probe executes 32 construction/work/fault/recovery/drop
cycles per profile. Actual Tokio resource rows, admission depth, crypto queue depth
and outstanding packet verdicts must drain. Embassy measurements unavailable
through the public metrics API are not invented. Timer queues must remain within
their explicit capacity. Every fixture releases all BLE connections. Full-node
traces separately account for deliberately retained static Embassy test wiring.

After each trace and fixture is dropped, retained heap must fit the measured
static retention plus an explicit 64 KiB auxiliary allowance. The dynamic peak
budget above that measured retention is 32 MiB. These are harness bounds, not
product memory requirements. The probe originally exposed oversized capture
reservation; the fixture now reserves 65,536 wire values and 131,072 medium events,
while requiring zero eviction.

## Verification record

Qualification and verification results are recorded here after the final source
checks. Physical boards, other operating systems and ISA execution are not part
of this host qualification.
