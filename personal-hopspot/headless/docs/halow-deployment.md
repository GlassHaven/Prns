# HaLoW appliance deployment

The first user experience should start in the web installer: choose the exact
ThinkNode G4 or Heltec HT-HD01-V2, download a verified Linux application bundle,
and follow a guided Ethernet/SSH installation. Keep the vendor operating system,
Morse firmware, regulatory configuration and calibration. These are application
targets, separate from firmware-upgrade targets.

This is an implementation specification for the next installation slice, not a
qualified persistent installer. The [desk qualification](qualification/halow-appliance-2026-10-01.md)
records working transport/control behavior and the recovery gap found on hardware.

## Compatibility and budgets

| Property | ThinkNode G4 inspected | Two Heltec HT-HD01-V2 inspected |
| --- | --- | --- |
| Board identifier | `morse,ekh03v3` | `Heltec,HT-HD01-V2` |
| CPU/application ABI | MT7628AN; static MIPS32r2 little-endian O32, soft float | Same |
| Kernel | 5.15.150 | 5.15.167 |
| Vendor image | OpenWrt 1.1 Morse-2.6.13 | OpenWrt 23.05.5, 2.8.5-20250924 |
| Writable overlay available at inspection | 5,336 KiB | 5,288 / 5,272 KiB |
| Common tested application | 3,620,680 bytes | Same executable and hash |
| Candidate gzip, level 9 | 1,501,636 bytes | Same payload; unpacking tested on G4 only |

Two compressed candidates total 3,003,272 bytes. This makes compressed flash
slots with RAM expansion a promising storage design; two unpacked executables
do not fit these overlays. Do not assume a newer application or another vendor
image has the same budget. The overlay must also accommodate private state,
configuration, update metadata and filesystem overhead. Updates need staging
headroom in RAM and explicit low-space refusal before writing flash.

G4 RAM-only gzip expansion, SHA-256 comparison and executable version invocation
passed. The board's coarse seconds counter advanced by two seconds across
expansion. This is not a boot-time benchmark or power-loss/update qualification.
Compression is storage encoding, not signature verification. Signed metadata must
identify both compressed and unpacked hashes and exact compatible board profiles.

## Installation transaction

One installer engine should own these named states; the website and a local
helper present its structured progress rather than reimplementing the transaction.

1. **Inspected:** read the board identifier, image/kernel, CPU ABI, regulatory
   region, radio devices, wired management path, listeners and actual RAM/overlay
   budgets. Reject unsupported combinations and pending vendor configuration edits.
   Discover addresses; do not bake in a factory IP or password. Verify SSH host keys.
2. **BackedUp:** export configuration and existing private Hopspot state privately.
   Record hashes and a restoration procedure; do not include credentials or board
   calibration in shared qualification output.
3. **Staged:** verify release signatures and compatibility locally, upload into a
   new private RAM directory, verify hashes on-device, then expand and verify the
   executable. Partial uploads never become an active version.
4. **Qualified:** arm an independent rollback lease before changing radio settings.
   Preserve Ethernet management. Start a bounded candidate with separate state and
   narrow lab grants. Require a real authenticated control exchange and an exact
   Resource/page exchange; a PID, listening port or mesh association is insufficient.
5. **Activated:** persist a verified version, configuration and supervised launch
   transaction. Keep identity and authorization state outside replaceable slots.
   Resolve radio-device readiness and recreation before claiming appliance readiness.
6. **Verified:** check the active application, unchanged identities, control/page
   exchanges and the retained previous version. Cancel rollback only after these
   checks. A failure restores the previous version and radio configuration.

Installation, updating and recovery use this same owner. No generic remote shell
operation is added to PRNS Remote Control. Avoid sourcing an untrusted downloaded
shell script or accepting arbitrary helper commands. Offline users should be able
to download the complete bundle and perform the same checks through documented SSH
steps. A failed health check needs a locally named stage; a timeout does not prove
an authorization denial.

## Radio and controller configuration

The current US lab profile is open 802.11s, 924 MHz center, 8 MHz, MCS2, long guard
interval, power saving off and mesh forwarding off. Region and legal power are
required inputs checked against the actual board; the desk run used 18 dBm.
Keep the profile fixed and editable. Do not silently apply the US channel to a
different regional SKU. Vendor `iw` output uses synthetic VHT channel/rate labels;
validate frequency/width through Morse tooling, without continuous intrusive polling.

Persist one explicit, stable local HaLoW scope across interface renames and
application updates. Source MAC supplies immediate neighbor identity without a
station-list polling dependency. MAC identity is transport addressing; controller
and node authentication remain cryptographic. Never clone private node state
between appliances.

Use `--tcp-mode gateway` when connected wired clients need discovery of destinations
behind the appliance. PointToPoint remains the CLI default. Gateway changes
discovery forwarding, not authorization. It does not establish that cold discovery
will cross every multi-hop topology; that needs separate routing qualification.

Enroll the controller's full public key and return the target's public key through
the trusted installation connection. Pin the target key in the controller. Give
inspection explicitly; app messages and interface watch each require separate
grants. Private controller keys remain on the controller. Retained grants require
deliberate revocation; omitting a CLI key is not a general revocation mechanism.

There is no automatic application announcement policy. For an entirely fresh lab,
the controller explicitly announces each node page through its wired connection.
That seeds HaLoW neighbors. Subsequent control can route across the radio; cold
control-endpoint discovery was qualified with these neighbors established. A quiet
mesh association alone does not populate PRNS MAC peers. Announcements, including
ordinary relays, use the shared broadcast channel; directed traffic uses unicast.

## Web-led entry and later automation

The initial website offers a development guide for each exact appliance, with
download/guided SSH as the first supported transport once signed bundles exist.
The USB Ethernet adapters here expose a network connection, not the remote board's
flash. [WebUSB needs an available claimable interface](https://developer.chrome.com/docs/capabilities/build-for-webusb);
the observed running G4 exposes no USB device-controller interface.

Browser-driven installation is still plausible through an authenticated device
endpoint or a local helper that runs the same installer engine. This adds its own
origin, credential and session-token contract. Chrome's
[Local Network Access permissions](https://developer.chrome.com/blog/local-network-access)
can gate public-site requests to LAN/loopback destinations; browser support and
transport-specific restrictions need actual matrix tests. Those permissions grant
network access, not installation authority. A public WebSocket Reticulum listener
must not become the installer endpoint.

## Remaining qualification gates

- Linux network-device recreation currently breaks over-air control while the
  process can remain available on wired TCP. A one-board application restart did
  not recover the observed retained-route exchange. Qualify binding replacement,
  routing recovery and bounded retry behavior together before shipping.
- Exercise procd start/stop, radio readiness, signal ordering and bounded crash
  retries. [OpenWrt's procd service documentation](https://openwrt.org/docs/techref/procd)
  supplies the integration foundation; a service file alone is not qualification.
- Test real reboot and interrupted activation/update at each transaction stage,
  low space, corrupt candidate/identity, identity retention and rollback to the
  previous signed version. Confirm flash persistence budgets and write frequency;
  do not discard authorization snapshots merely to avoid route writes.
- Qualify least privilege, bounded logs/state, sustained actual PRNS Resource
  throughput, longer desk runs, physical forced multi-hop and field range.
  Vendor LED/button polling remains active in the latest desk checks; replacing
  it needs preserved button behavior, not a shipped `SIGSTOP` trick.
- Keep browser assets, Auto-WiFi and WebSockets optional until their additional
  flash/RAM and coexistence budgets fit. ESP-NOW interoperation is a separate
  unproven capability on these Linux radios.
