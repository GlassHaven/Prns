# Response-size accounting

Status: packet and whole Resource accounting corrected, including metadata-bearing
files; segmented Resource accounting remains open.

## Completed packet slice

Packet responses now count exactly the bytes delivered after removing the outer
request-ID envelope. Values are opaque to this layer: bin8/bin16/bin32 headers
count in full, and a nil value counts as one byte. Zero is a real bound; unlimited
remains explicit. Neither wire encoding nor adapter decoding changed.

The owner tests cover exact boundaries, arbitrary values, binary-header-shaped
bytes, empty-to-nil encoding, one terminal result, receipt cleanup, and continued
link use. The mixed-runtime BLE tests reproduce the old discrepancy (Tokio
accepted 257 bytes under a 256-byte limit while Embassy's completion buffer
refused it) and now prove the shared-core refusal on both endpoint pairings.

Adapter audit: Tokio and Embassy retain the encoded value unchanged. The native
host and legacy N-API request wrapper subsequently unwrap complete binary values;
their limit still applies before decoding. N-API's `packed.length` is therefore
the packet budget, not `data.length`. WASM's engine event projection retains the
encoded bytes. Native/C and N-API fixture limits now include binary value headers.

## Completed whole-Resource slice

Whole, metadata-free Resources now enforce the same encoded-value limit as
packets. Advertisement admission discounts at most the fixed outer response
envelope using saturating subtraction; existing transfer/storage ceilings still
apply. Conclusion checks the actual value before any delivery, including legacy
raw bodies with no prefix to discount and bodies resumed after decompression.
Wrong enclosed request IDs settle as `ResponseTransferFailed(TransferCorrupt)`.
Whole uncompressed streams must match their advertised length, as inflated whole
streams already had to do.

Owner tests cover canonical and legacy forms, compression, false lengths,
zero/exact/overflow/unlimited limits, terminal settlement and retired state.
Mixed-runtime BLE scenarios now accept exact 1,200-byte response-value budgets
and fill Embassy's existing 2 KiB completion capacity. They still refuse an
oversized response and reuse the same links afterward. No shipping buffer or
queue capacity changed. See [verification evidence](../../validation/simulation/measurements/whole-resource-response-limits.md).

## Original observation

The mixed Tokio/Embassy Resource capstone found that a 1,200-byte application
response is rejected with `maximum_response_bytes = 1200`, but succeeds when the
limit includes `RESPONSE_WIRE_OVERHEAD`. That original test deliberately named
it an envelope limit; the corrected test now asserts response-value limits.

- [Resource admission](../src/routing/links/resources/receive/gate.rs) previously
  compared all advertised uncompressed `data_bytes` against the request limit.
- [Packet admission](../src/routing/ingress/links.rs) previously subtracted two
  bytes unconditionally. It now uses the complete response value's length.
- [Resource conclusion](../src/routing/links/resources/receive/conclude.rs) strips
  the response envelope, with compatibility for legacy raw-body responses.
  Metadata and segmented responses have additional accounting paths.
- Embassy completion buffers retain application response bytes, not the outer
  request/response envelope. Their capacity is also used as the core limit;
  discounting the outer envelope restores the full usable capacity for whole,
  metadata-free responses.

## Completed whole-file slice

Whole metadata-bearing responses count literal file bytes after removing the
verified metadata block. Advertisement admission cannot know metadata length,
so the final value check runs after verification/inflation; the existing
uncompressed-stream, fixed-transfer and heap-memory ceilings still apply.
Files remain literal even when their leading bytes resemble a response envelope,
including the first segment of a split file. This matches the metadata-bearing
response path in the pinned RNS 1.4.2 and 1.5.0 implementations.

Owner tests cover metadata framing, arbitrary file bytes, exact bounds,
compression, malformed metadata, allocation ceilings and retired state. Mixed
Tokio/Embassy BLE scenarios exercise the shared static-file command, exact file
budgets and Embassy's existing 2 KiB completion buffer. See
[verification evidence](../../validation/simulation/measurements/metadata-resource-response-limits.md).

## Completed split-failure prerequisite

A mismatched request ID in the first split-response envelope now fails the request
as `ResponseTransferFailed(TransferCorrupt)`. Previously that segment was silently
omitted while assembly continued, allowing the tail alone to settle successfully.
Both normal opening and resumed decompression propagate the failure before
advancing the assembly. Failed admitted split transfers also release their assembly state,
including cancellation, malformed metadata, refused hashmaps and exhausted retries.
Whole-transfer failures do not erase a separate split assembly waiting on the link.

Buffered Tokio and Embassy request APIs discard provisional chunks on failure;
tests also prove the next request can reuse the awaiter/slot. Direct journal
consumers must wait for successful final settlement before publishing the value.
No stored fields, capacities, wire formats or response-size admission bounds change.
See [verification evidence](../../validation/simulation/measurements/split-response-failures.md).

## Completed continuation-admission prerequisite

The assembly's stored segment count now constrains every continuation, alongside
its original hash and next index. Completed chains admit no extra segment, and
index comparison cannot wrap at `u64::MAX`. Admission rechecks this same contract
after a queue wait or application decision, before allocating transfer buffers or
claiming a response receipt. A stale queued continuation is removed without
changing the current assembly or settling its request; a still-valid queued
continuation resumes normally. No retained fields, capacities or wire formats change.

See [verification evidence](../../validation/simulation/measurements/split-continuation-admission.md).

## Completed link-ownership prerequisite

Request receipt lookup now requires both the authenticated link and the request
ID. Previously, another link could name a live request and deliver a packet
response, claim its timeout through a Resource offer, or settle it as failed.
All receipt reads, claims, timeout changes and settlements now share this
link-scoped lookup, including delayed Resource completion and pending offers.
The receipt already stores the owning link; no retained fields, capacities or
wire formats change. Public receipt methods now require `&LinkId`; there is no
ID-only compatibility alias. All in-repository callers have been migrated.

Fixed/heap receipt tests cover both lookup-key components, colliding IDs on
separate links, whole policy snapshots and exact settlement. Core-engine tests
use two authenticated links and prove that wrong-link packets, offers and late
cancellations leave the other request able to complete. This is cross-link
isolation, not yet same-link split-chain correlation or cumulative accounting.
See [verification evidence](../../validation/simulation/measurements/response-link-ownership.md).

## Completed split-position ownership prerequisite

Each admitted transfer now retains its offered original chain hash. Opening and
resumed decompression verify that hash, the segment index and total count against
the current assembly before proving or publishing bytes. A superseded transfer
fails as `TransferCorrupt`; its cleanup cannot clear an unrelated or advanced
assembly. Advancement itself also requires the exact named chain position.
The original hash is a required argument to `IncomingResources::accept`, and
`IncomingAssemblies::advance` now requires the original hash, index and count.
There are no permissive link-only compatibility methods for either operation.

This adds a 32-byte identity field to each incoming transfer state, without
changing buffer capacities or the wire. Overlapping-chain tests cover normal
opening, delayed decompression and cancellation, followed by exact completion of
the replacement response. This protects assembly position; the following slice
also binds request correlation within one link. See
[verification evidence](../../validation/simulation/measurements/split-position-ownership.md).

## Completed split-correlation prerequisite

Each assembly now retains its semantic association: unsolicited, request with ID,
or response with ID. Continuations must preserve that complete association at
initial admission, queued promotion, completion and advancement. Initial validation
runs before response policy so a retargeted oversized continuation cannot fail the
unrelated request it names. A stale split transfer also cannot settle the receipt
of a replacement chain answering the same request or change its deadline.

`IncomingAssemblies::begin`, `fit` and `advance` require an explicit
`AssemblyCorrelation`; fixed and heap tables retain it with the owning row. No
wire shape, buffer size or transfer capacity changes. Deterministic core-engine
tests reproduce retargeting and same-request replacement failure with encrypted
frames. The exact failure injections are not yet mixed-runtime simulator scenarios;
passing simulator suites are separate regression evidence. See
[verification evidence](../../validation/simulation/measurements/split-correlation-ownership.md).

## Remaining segmented scope

Extend the delivered-value byte-counting contract to segmented Resource forms.
Separate early allocation protection from final delivered-payload validation;
loosening an advertisement check alone would admit oversized legacy raw bodies.
Keep the authoritative accounting in shared core, with host completion buffers
enforcing their own actual storage capacity rather than compensating for wire
headers independently.

Cover exact limits, one-byte overflow, zero and unlimited bounds, raw bytes,
MessagePack bin8/bin16/bin32, canonical and legacy Resource envelopes, metadata,
compression, and multi-segment responses. No prefix may be silently subtracted
from arbitrary application data. Check overflow-safe advertised bounds and
bounded allocation before receiving an untrusted transfer.

Extend owner tests and the mixed-runtime capstones with exact Embassy completion
capacity boundaries. Verify typed refusal, no truncated or partial successful
response, one terminal result, receipt cleanup, and subsequent link usability.
Preserve Remote Control's fixed response bounds and stock-Reticulum wire
interoperability. Audit reusable native/Node.js/WASM client semantics before
publishing a changed limit contract; do not introduce a transport-specific fix.

Include assembly cleanup when a request expires between segments or a continuation
is refused before admission; the split-failure prerequisite handles admitted
transfers only. Segment counts and request correlation are now stable across the
chain; validate advertised stream totals before loosening admission. Arbitration
between a whole response and an overlapping split response naming the same request
is not addressed by the split-chain ownership checks.
