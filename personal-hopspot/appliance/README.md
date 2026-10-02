# OpenWrt application slots

This development slice owns signed application activation and supervised launch
on the inspected ThinkNode G4 and Heltec HT-HD01-V2 vendor images. It does not
flash firmware, alter UCI/radio settings, enroll controllers, announce, or update
the slot manager itself. A public installer is still gated on signing operations,
the remaining physical recovery checks and a complete guided enrollment flow.

Build with `./tools/prns run build.hopspot.appliance -- --zig PATH --output NEW_DIR`.
The shipping `manager` pins `release/keys/minisign.pub`. The separately requested
`--qualification-output PATH` builds an example accepting a temporary lab key;
it is never included in the bundle. Do not install that example for users.

## Package contract

Each private package directory contains `manifest.json`, its detached Minisign
signature `manifest.minisig`, and `app.gz`. The signed JSON has schema 1, a
path-safe `version`, one exact vendor `board`, ABI
`mips32r2-le-o32-soft-float-static`, and `compressed` / `executable` objects with
positive `bytes` and lowercase SHA-256 `sha256`. Verify the signature before
parsing metadata, then verify both artifact hashes, expanded length, static
ELF load headers and the MIPS soft-float ABI flags. Decompression is bounded by
the signed executable length and explicit local budgets.

An inspected root contains a fixed `manager`, an explicit `config.json`, slots
`a` / `b`, alternating checksummed journals and `state/`. The application alone
owns state identities and retained authorization. Updates never delete or copy
that state. Provision initial grants deliberately through the existing headless
CLI before enabling the service; the durable launch configuration cannot carry
enrollment arguments that would undo later revocation. Copying private state
between different appliances is prohibited.

## Guided activation

Keep private configuration/state backups and independent wired management. First
inspect `/tmp/sysinfo/board_name`, filesystem/RAM budgets and existing listeners.
Use the same explicitly supplied budgets for `stage`, `status`, `confirm`,
`rollback` and `run`; the example procd asset shows the minimal application's
current limits. It is a profile to qualify, not a universal device budget.

1. Upload a verified package into private RAM. `stage --package PATH
   --trial-launches 3` verifies it under the manager's trust and board policy,
   checks storage headroom, replaces only the inactive slot and publishes a
   durable trial journal. It returns the candidate revision and digest as JSON.
2. Install a reviewed launch config and the service asset. `run --config PATH
   --ram-directory /tmp/hopspot` consumes a durable trial launch before expanding
   the executable. It then uses Unix `exec`, preserving procd's PID and signal
   ownership. No radio presence is required for wired application readiness.
3. Through a pinned controller, verify actual build/interface snapshots, an
   authenticated app exchange and the exact node page/Resource. Inspect radio
   availability separately. `confirm --revision N --executable-sha256 DIGEST`
   accepts only that exact current trial. Confirmation is a trusted operator
   action following those checks; the manager does not infer health from a PID,
   socket, log line or association.
4. A failed candidate can be rolled back explicitly. Three unconfirmed launches
   exhaust the trial and select the verified previous application on the next
   launch. With no previous application, startup fails closed. Confirmed packages
   are reverified at every launch. A corrupt confirmed slot is never executed.

The service passes an explicit `respawn 60 2 3`: quick failures have bounded
retries, while a process surviving the threshold starts a fresh crash window.
Trial launch accounting also persists across boots. `term_timeout 15` permits
the application's SIGTERM flush. File descriptors are capped at 128 and core
dumps disabled. There is no netdev trigger that restarts the application during
radio churn. Root/CAP_NET_RAW and OS log rotation remain separate qualification
gates; do not claim least privilege from this service file.

## Recovery and evidence boundaries

Files are synchronized before publication; slot and journal renames are followed
by directory synchronization. Interrupted candidate staging cannot replace the
confirmed slot. Journal checksums detect torn JSON; the highest valid revision
wins. Two corrupt journals, or vanished journals beside installed slots, refuse
startup. A failed write poisons the writer until it is reopened, because a failed
directory sync may have followed a visible rename. Concurrent writers are locked.

Native cut-point tests model interruption after each observed publication step,
trial exhaustion, stale confirmation, corrupted packages/journals and unchanged
state. They are filesystem/process fault evidence, not actual flash power cuts.
The filesystem transaction is qualified on Unix filesystems; Windows browser or
local-helper clients delegate activation to the appliance's Linux manager. The
signature/package contract and activation vocabulary remain portable Rust.
Real power-loss behavior, manager upgrades, anti-downgrade policy, signing-key
rotation and a browser/local-helper transport remain subsequent gates. The app
signature authenticates publisher intent; the local journal checksum is not a
signature or protection against an attacker with root access.
