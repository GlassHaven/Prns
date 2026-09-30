# Native HaLoW datagram backend

`wifi-halow` exposes the attachable `HaLow` supervisor and, on Linux,
`HaLowSocket`. Attach with `handle.supervise(HaLow::new(socket, instance,
peer_policy, broadcast_policy, limits))`. The caller provides a configured radio,
a stable instance tag, distinct pacing policies, and nonzero peer/idle limits.
Radio configuration and headless CLI integration remain separate work. The core
`wifi_halow` module owns source-MAC identity and framing without allocation or OS APIs.

## Runtime contract

The supervisor attaches one `WifiHaLowBroadcast` channel immediately and one
`WifiHaLowPeer` channel per source MAC on receipt of a valid envelope. The original
frame enters that peer's receive queue without a handshake or station-table poll.
Reticulum validates the enclosed frame and authenticates identities afterward;
a valid envelope alone is not authenticated admission.

Unrestricted announces use the shared channel once per configured radio, including
with zero peers. Existing runtime announce pacing and backpressure apply to that
channel. Directed announces stay unicast. `AllExcept(peer)` uses unicast for that
peer's radio so the excluded peer is not an intended recipient; other radios can
still broadcast. Thus some relayed announces are directed fan-out. Ordinary
non-announce fleet traffic always fans out over known unicast peers. Direct
traffic never falls back to broadcast. No payload-hash or timer deduplication is used.

The experimental payload is `PRNSHL`, version byte `1`, reserved byte `0`, a
big-endian u16 frame length, and exactly that frame. Only minimum Ethernet padding
(to 46 payload bytes) may trail a short frame. The Ethernet payload cap is 1500
bytes; Prns frames occupy at most 1490, with advertised link MTU capped at 1426
so the maximum 64-byte IFAC still fits. The caller must choose the same EtherType
and wire version at both ends. `0x88b6` remains a lab value, not a Prns assignment.

Peer admission has a caller-selected cap of at most 255 peers, 16 queued datagrams
per peer, and no allocation on a full receive queue. New sources at capacity and
frames for full queues are dropped. Both received and successfully kernel-accepted
sent frames refresh idle expiry; housekeeping runs every five seconds. Successful
TX does not establish remote liveness. Send backpressure has a two-second deadline.
A bounded receive burst yields to other work. Runtime egress queues are separately
bounded. A fatal receive error ends the supervisor and detaches its children;
automatic socket rebinding is not implemented yet.

A portable injected-datagram test drives a real Prns node through zero-peer
announcement, malformed-envelope rejection, first-frame admission, directed reply,
no duplicate broadcast, capacity, TX-refreshed expiry, and supervisor teardown.
Another covers TX timeout, failure, oversize rejection, and no broadcast fallback.
These tests do not qualify the new framing/supervisor on hardware.

The backend uses `AF_PACKET`/`SOCK_DGRAM`, an explicit device binding and EtherType,
and requires `CAP_NET_RAW`. Opening it does not change radio configuration or
enable promiscuity. Broadcast sends one Ethernet group frame; directed sends use
a validated unicast MAC. Success means kernel acceptance, not receiver delivery.

Receive uses the `ETH_P_ALL` ingress tap with a BPF EtherType filter installed
before binding. Protocol-specific sockets on a bridge port can miss frames that
the bridge consumes. The filter excludes unrelated protocols in the kernel.
Userspace rejects wrong interfaces/protocols, outgoing echoes, other-host and
multicast destinations, malformed addresses, truncated frames, and invalid peer
source MACs. It yields after a bounded burst of discarded packets.

`receive()` supplies the source MAC on the first frame. `InstanceTag::peer_id`
derives its scoped identity without polling or a discovery handshake. A changed
MAC creates a new transport peer; MACs do not authenticate Reticulum identities.

## Hardware evidence, 2026-09-29

The statically cross-compiled `halow_datagram` example was uploaded to RAM on the
G4 and first Heltec with device-side SHA-256 verification. They retained their
existing AP/station configuration; there was no mesh or third-node test this round.

The initial protocol-bound socket received 4/5 broadcasts, then 5/5 on repeat,
but 0/5 reverse unicasts. Independent Ethernet capture showed all five unicasts
arriving. With the filtered ingress tap, the same test received **5/5 broadcast
and 5/5 reverse unicast**, deriving stable interface IDs immediately from source
MACs. Both radio health checks passed afterward and wireless UCI changes were
empty. The earlier broadcast loss remains part of the evidence.

Tested executable SHA-256:
`e08cd849303ac872bccbd6bdf51ef0451ff99d9c73b33b22b20a17f61e29c57d`.
These are tiny datagram smokes, not throughput, field loss rates, over-air
single-send fan-out, or end-to-end Reticulum qualification. EtherType `0x88b6`
and the smoke payload marker are local experiments, not a production assignment.

The Ethernet metadata rejection unit test also passed on the G4 MIPS CPU.
463 core interface tests passed, including scoped peer identity and radio status
serialization, along with the core no-default-features check.

On an already configured device, the example accepts `<device> receive`,
`<device> broadcast`, or `<device> unicast <peer-mac>`. Reception lasts at most
15 seconds; transmission sends five numbered payloads with per-send deadlines.
The Linux CI check is
`python3 validation/run.py run --suite linux-halow-data-plane`; it requires no
radio or packet-socket privileges and does not transmit.

References: [Linux packet sockets](https://man7.org/linux/man-pages/man7/packet.7.html)
and the [Linux bridge receive path](https://github.com/torvalds/linux/blob/v5.15/net/bridge/br_input.c).
