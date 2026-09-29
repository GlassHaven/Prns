# HaLoW Hopspot integration

This records the implementation contract and remaining work after the
2026-09-29 three-device experiments. The running headless application is still
TCP-only. The G4 build task and web installation guide exist; the native HaLoW
transport below is a proposed next implementation, not an available feature.

## Product shape

One headless Hopspot process owns its Reticulum identity, routing, persistence,
and attached interfaces. OpenWrt owns radio configuration, the network devices,
and service supervision. Install the application over the existing vendor OS.
Keep Morse firmware, drivers, board calibration, and regulatory data in place.

The web flasher is the user's starting point. The G4 entry initially explains
the application installation and provides the development build procedure.
A public download requires integration with signed release custody and hardware
qualification. Never label an arbitrary ELF as a firmware image or offer it to
LuCI's firmware upgrader.

## Native HaLoW contract

Use the Linux mesh network device's normal Ethernet data service, with one
shared packet socket. The lab used `AF_PACKET`/`SOCK_DGRAM`; no monitor injection
or custom chip firmware is needed for this path. Linux supplies the source MAC
alongside the received data. Keep platform FFI in `prns-ffi`, asynchronous I/O
in `prns-interfaces-tokio`, and transport policy independent of Linux.

The proposed peer key is a length-delimited stable local interface instance tag
plus the remote source MAC. Hash that with a new peer `InterfaceKind` through
the existing `InterfaceId::from_channel_tag` contract. Do not use the transient
Linux interface index. This identity choice is awaiting the user's explicit
confirmation before implementation.

A valid first frame from an unseen source must attach the peer and deliver that
same frame in order. It must not wait for a station-table polling interval or a
new transport handshake. Bound peer admission and pending work. Reject malformed
frames, multicast source addresses, local outgoing echoes, and unrelated
EtherTypes before creating peers. MAC addresses identify transport neighbors;
Reticulum's cryptographic identities provide authentication. A changed MAC is
a new transport peer, and the same MAC on two configured local instances stays
distinct. Mesh peer events or occasional reconciliation inform liveness, not
whether an incoming frame has an identity.

Announces use one physical broadcast. Traffic with a particular destination
uses that peer's unicast MAC, retaining the driver's normal acknowledgements and
aggregation. A direct transmission must never silently fall back to broadcast
when its peer disappears. Announce fan-out must work even with zero known peers,
otherwise first discovery depends on already having discovered someone.

The current Tokio egress expands fleet announcements into a per-peer loop.
Introduce a shared-medium egress capability before that expansion. Preserve the
engine's original intent through pacing and backpressure; do not try to recover
it afterward with payload hashes or timing-based duplicate suppression. Account
and pace physical broadcasts once per radio instance, and keep direct queues
bounded and fair. Compatibility with existing point-to-point fleets matters.

The exact handling of `FanTarget::Only`, `AllExcept`, and non-announce fan-out
must be explicit. A physical broadcast cannot exclude one receiving station.
An excluded peer can physically hear it; routing suppression and duplicate
handling must prevent unwanted forwarding. Restricted recipient semantics that
actually require exclusion need directed delivery. Path requests and other
control fan-out need their own deliberate mapping; do not classify all fan-out
as an announce or all unicast data as an aggregate.

Keep 802.11s forwarding disabled for the initial Prns neighbor interface so the
overlay owns multi-hop forwarding. Enabling it later changes which endpoints a
MAC represents and needs separate qualification. The first implementation must
also specify a versioned wire discriminator and frame/MTU bounds: the lab's
experimental EtherType `0x88b6` is not a registered Prns assignment.

## Initial radio profile

The selected lab default is an open 802.11s mesh at 924 MHz center, 8 MHz width,
MCS2, long guard interval, power saving off, and mesh forwarding off. Keep the
profile explicit and editable after installation. Region and legal transmit
power must be validated against the actual board's regulatory configuration;
924 MHz is not a worldwide default. Do not dynamically negotiate a different
profile in this first implementation.

Measured receive payload goodput with vendor polling loops paused:

| Setting | Nominal PHY, long GI | Broadcast | Unicast |
| --- | ---: | ---: | ---: |
| MCS2 | 8.775 Mbps | 4.07–4.19 Mbps | 7.32–7.51 Mbps |
| MCS3 | 11.7 Mbps | 4.83–4.84 Mbps | 9.99–10.06 Mbps |
| MCS4 | 17.55 Mbps | 5.60–5.63 Mbps | 14.29–14.31 Mbps |

These are close-range, 1,400-byte probe results, not encrypted Reticulum resource
throughput or field-range results. A second receiver lost 2.955% in one MCS2
broadcast trial. See `scratch/halow-mcs234-2026-09-29/README.md` for the experiment
conditions and raw evidence in the development checkout. Shipping qualification
must retain sanitized evidence in durable release artifacts.

Vendor LED/button shell polling consumed appreciable host CPU. Production
integration should replace or properly disable the identified services while
preserving required button behavior; `SIGSTOP` is an experiment, not an installer
policy. Avoid hot-loop polling and periodic intrusive firmware-stat reads. Use
bounded queues and expose drops, airtime-relevant frame counts, resource usage,
and restart causes. Benchmark actual Prns transfers after raw-link qualification.

## Additional interfaces on the same appliance

| Capability | Existing foundation | Remaining appliance work |
| --- | --- | --- |
| Wired TCP | Headless host and lifecycle tests | Persistent service and install transaction |
| Wi-Fi AP or client | G4 reports both modes; vendor OpenWrt manages them | Explicit choice, retained Ethernet management, DHCP/firewall and concurrency checks |
| Auto-WiFi and mDNS | Tokio interface family and discovery support | Bind to chosen networks; qualify multicast on AP/client; budget sockets and discovery traffic |
| WebSocket and browser rendezvous | Tokio WebSocket and browser-rendezvous modules | Minimal MIPS feature set, listener policy, browser interoperability and resource limits |
| Locally served browser node | Existing browser runtime/playground | Offline asset packaging, storage budget, origin/security-context checks, lifecycle tests |
| ESP-NOW | Existing embedded support and 2.4 GHz Public Action TX/RX evidence | Linux vendor-category TX/RX and real Espressif interoperability remain unproven |

AP and client operation on one 2.4 GHz radio may share a channel and airtime.
ESP-NOW coexistence must respect that same channel. The HaLoW radio is separate.
The earlier 2.4 GHz test delivered all five short Public Action payloads without
association, but normal vendor-category 127 transmission was rejected. A later
combined raw-injection experiment rebooted the G4; the evidence does not identify
which radio or stage caused it. Public Action success alone does not establish
ESP-NOW compatibility. Further injection work needs crash visibility first.
No Bluetooth or LoRa capability should be inferred from these boards without
the corresponding hardware. Do not bundle every optional transport into the
minimal image until its code, RAM, and flash costs are measured.

The G4 currently has about 5.2 MiB free overlay. The static TCP-only application
is about 3.2 MiB. An update requiring two full copies and additional browser
assets does not fit that budget. Qualify an actual storage/update strategy before
advertising persistent installation with rollback. Keep private identity outside
replaceable application directories; bound logs and persisted routing state.

## Browser installation investigation

The connected G4 is reached through the Mac's built-in Ethernet. The two USB
network adapters enumerate as Realtek and ASIX networking devices. A WCH
`1a86:7523` serial adapter also enumerates, but its connection to a particular
board has not been established. No serial session was opened. The running G4
image has no `/sys/class/udc` device-controller interface.

[WebUSB requires a claimable USB interface](https://developer.chrome.com/docs/capabilities/build-for-webusb).
An Ethernet adapter does not provide access to the remote board's flash, and the
host network driver already owns its networking interface. A future USB gadget
or known UART may offer another route, but neither is currently qualified here.

Browser HTTP or WebSocket access could drive an authenticated local device API
or a loopback helper. Ordinary web pages do not have a raw SSH socket API.
Cross-origin access, authentication, mixed-content rules, and evolving
[local-network permissions](https://developer.mozilla.org/en-US/docs/Web/Security/Defenses/Local_network_access)
must be tested in each supported browser. Serving the browser app locally can
simplify origins, but plain LAN HTTP is not a secure context for every web API.
Public visitors' browser-node access and administrative installation must remain
separate privileges. Preserve signature verification and origin restrictions in
any helper; do not turn it into an unauthenticated local command proxy.

## Implementation gates

1. Confirm MAC-based peer identity. Add shared-radio egress and first-frame
   admission tests before writing the Linux adapter around that contract.
2. Prove one broadcast reaches both Heltecs, while direct frames address one
   peer, including startup with no peers, first unicast without a peer-list
   entry, churn, restart, saturation, malformed input, and receiver exclusion.
3. Fetch the actual Hopspot page and transfer a resource across native HaLoW;
   then route through a different interface and a three-node topology.
4. Add AP/client discovery and WebSocket/browser access in measured increments.
   Keep ESP-NOW qualification separate from ordinary Wi-Fi operation.
5. Qualify persistent installation, identity retention across power loss, bounded
   state growth, full-storage behavior, and update rollback. Then publish a signed
   application download and promote the web entry out of development preview.
