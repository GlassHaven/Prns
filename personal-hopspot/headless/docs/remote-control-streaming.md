# Remote Control streaming work

The first streaming foundation is in `prns-core::remote_control::stream` and the
Tokio byte-stream receive path. It is not yet an advertised Remote Control
capability or a controller-facing subscription. Existing bounded `AppMessage`
requests remain request/response operations.

The event frame is version 1 and exactly 14 bytes: version (1), kind (1),
sequence (4, big-endian), interface ID (8). `InterfaceChanged` and
`PeersChanged` invalidate the corresponding paged snapshots. `ResyncRequired`
has a zero interface ID and tells the controller to refetch all snapshots.
Events contain no partial counters or fabricated radio readings. A sequence gap
requires the same full resync. Sequence numbers are scoped to one subscription,
so reconnect starts with a fresh full snapshot.

The Tokio byte-stream reader now has a 32-frame receive queue. If the manifold
cannot enqueue a frame, it sends a typed `Overflowed` outcome; the reader returns
an `InvalidData` error that retains that cause instead of exposing a truncated
stream. Link closure and source shutdown have separate terminal outcomes. The
sender continues to use the channel's existing send window. This protects memory
and loss detection, but does not itself
provide a Remote Control event producer or subscription.

The next slice is a separately granted `WatchInterfaces` request, admitted after
the controller's identity has been verified. The host should advertise it only
when a producer is installed. Admission failures must remain silent. The
controller should choose a stream ID and register its reader before sending the
subscription request; the authenticated response accepts that ID. Limit active subscriptions and buffered
events per node; coalesce repeated invalidations and send `ResyncRequired` if
the producer cannot preserve a complete sequence. Closing the controller handle
must cancel its subscription. Verify slow-reader, link-loss, reconnect, grant
revocation, and multi-controller behavior before enabling it on the G4 and
Heltec lab hosts.
