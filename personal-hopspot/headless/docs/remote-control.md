# Controller-operated Hopspots

Hopspots do not schedule application announcements at startup, on reconnect, or on
a timer. A controller or explicit local user action chooses when to announce.
Transport discovery (Auto-WiFi beacons, DNS-SD) and responses to requested paths
remain necessary protocol operations; they are not application announcement policy.

## Provision and operate

Build the native lab controller:

```sh
cargo build --manifest-path personal-hopspot/headless/Cargo.toml --example controller
controller --state-dir /private/controller-state identity
```

Use the built example executable in place of `controller`. Retain this private
state directory: restarting the tool must not change the controller identity.
The output contains its public identity hash and full public key. Provision the
**full public key**, not a password or the hash alone, on each target:

```sh
personal-hopspot-headless --state-dir /persistent/hopspot-state \
  --listen 0.0.0.0:4242 --controller-public-key "$CONTROLLER_PUBLIC_KEY"
```

Repeat `--controller-public-key` for additional controllers. The initial grant is
Operator with `Describe`, `AnnounceSelf`, `DescribeBuild`, `InventoryInterfaces`,
`InventoryInterfaceConfig`, and `InventoryInterfacePeers`; it cannot administer grants
or mutate interfaces. For a lab controller that also needs the bounded application
probe, use `--app-controller-public-key` instead. Do not provision the same key with
both options. `AppMessage` is a separate grant and is never implied by inspection.
On fresh state, no option means nobody is authorized. These are initial grants;
the runtime can also restore a retained authorization snapshot. Omitting a CLI
key is not a general revocation mechanism for previously persisted grants. The
ready line reports the number of CLI grants, not an effective grant inventory.
Retain the option in the service launch
configuration across restarts. The target's separate remote-control identity is
persisted under `STATE_DIR/remote_control`. Never copy private controller keys
onto targets. Back up private state securely; share only public provisioning data.

Read `target_key` from the target's `hopspot_remote_control_ready` line through a
trusted provisioning channel. Pin it when invoking the controller:

```sh
controller --state-dir /private/controller-state invoke \
  --tcp 192.168.12.1:4242 --target-key "$TARGET_PUBLIC_KEY" --action describe
controller --state-dir /private/controller-state invoke \
  --tcp 192.168.12.1:4242 --target-key "$TARGET_PUBLIC_KEY" --action announce
controller --state-dir /private/controller-state invoke \
  --tcp 192.168.12.1:4242 --target-key "$TARGET_PUBLIC_KEY" --action build
controller --state-dir /private/controller-state invoke \
  --tcp 192.168.12.1:4242 --target-key "$TARGET_PUBLIC_KEY" --action interfaces
controller --state-dir /private/controller-state invoke \
  --tcp 192.168.12.1:4242 --target-key "$TARGET_PUBLIC_KEY" \
  --action peers --interface-id "$INTERFACE_ID"
controller --state-dir /private/controller-state invoke \
  --tcp 192.168.12.1:4242 --target-key "$TARGET_PUBLIC_KEY" \
  --action config --interface-id "$INTERFACE_ID"
controller --state-dir /private/controller-state invoke \
  --tcp 192.168.12.1:4242 --target-key "$TARGET_PUBLIC_KEY" \
  --action app-message --message-hex 0101
```

The example demonstrates the public API sequence: provision trusted target access,
request its control endpoint path, connect and identify, then invoke one typed operation.
Output records are JSON lines on stdout; diagnostics and errors go to stderr. Inventory
and peer results are paged. `--interface-id` is an eight-byte hex identifier taken from
the interface inventory. The application probe uses version byte `01`, operation byte
`01`, and responds with those bytes followed by the verified 16-byte controller hash.
Only an explicitly app-granted controller can reach the handler. Both request and reply
are limited to 96 bytes. Other probe payloads produce an app-level `ApplyFailed` response.
Cold-start control does not require the target to periodically
announce first. The command has a bounded lifetime and closes its link afterward.
The target's configured self-announcement destination is its node page. HaLoW
announcement delivery continues to use the shared physical broadcast channel;
ordinary directed traffic uses peer channels.

## Current limits and next API work

- An unauthorized request currently manifests as an exchange timeout in the lab.
  Do not label every timeout as an authorization failure: disconnected targets and
  packet loss are also possible. Preserve silent rejection as the accepted security
  policy: `RemoteControlAdmitError` maps every denial to `Decline::Ignore`. Improve
  controller-local diagnostics without adding a denial response or claiming that
  a timeout proves an authorization failure.
- `AnnounceSelf` covers one configured application destination. It does not select
  a destination set or interfaces, and does not currently announce the separate
  delivery destination. Any broader operation needs an explicit typed API.
- The Linux host currently exposes read-only build/interface/config/peer snapshots.
  A missing radio reading is reported as `Unavailable`. The existing inventory wire
  format uses zero for an absent transfer-rate estimate; the typed API represents it
  as `None`, and the example emits JSON `null`. Do not read an available estimate as
  measured throughput. Radio configuration and service management are later slices.
- The example takes the target key and endpoint explicitly. A durable controller
  target registry would improve repeated lab use.
- Streaming groundwork now includes a versioned invalidation frame and a bounded
  Tokio byte-stream receive queue that reports overflow. The subscription request,
  separate stream grant, server producer, controller watch API, and device qualification
  remain to be implemented. See [the stream contract](remote-control-streaming.md).

## Deployment status

The application, static MIPS build, HaLoW adapter, WebSocket listener and Auto-WiFi
transport have bounded qualifications. This controller workflow replaces the lab
announcement ticker; it is not yet a persistent appliance installer. Remaining
work includes service/config installation, controller enrollment UX, updates and
rollback, resource qualification, and a web-led application installer. ESP-NOW
interoperability remains separate qualification work. Do not infer completed
radio management or browser provisioning from the available transport APIs.
