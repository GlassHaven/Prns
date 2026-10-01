# T1000-E firmware recovery

Hopspot keeps the T1000-E stock bootloader and SoftDevice intact. Recovery
entry requests a reset into that bootloader; it does not erase settings or
install another application.

The following shortcuts require a Hopspot build containing recovery entry.
Older installed builds still need the vendor sequence described below. The
startup shortcut also needs the application to reach hardware initialization;
it cannot recover a damaged bootloader or an application that never starts.

## From the flasher

Open the T1000-E page of the Hopspot flasher in current desktop Chrome or Edge,
connect the tracker, and select **Switch firmware → Enter recovery mode**.
Choose the Personal Hopspot device in the USB picker. This action works without
preparing a Hopspot release or downloading firmware.

The browser reports a request acknowledgement. Confirm that the `T1000-E`
drive actually appears and that `INFO_UF2.TXT` identifies the T1000-E before
copying firmware. If the request is rejected, use manual recovery; older
firmware does not implement this request. Close other apps connected to the
tracker if the browser cannot claim its USB interface.

## With the button

Leave the USB end of the cable connected to the computer. Remove the magnetic
connector, hold the upper button near the lanyard, and attach the connector
once. Keep holding through the hardware reset and startup until the recovery
drive appears; allow at least six seconds before releasing.

Hopspot checks the active-high button on P0.06 immediately after HAL
initialization. If USB power is present and the button remains held for two
seconds, it enters UF2 recovery before initializing persistent storage,
identity, radio, or the application's USB interface. Normal startup is not
delayed when the button is released. This startup shortcut is implemented but
still needs a physical held-button qualification before release.

For older firmware or an unresponsive application, hold the same button while
rapidly attaching, removing, and reattaching the magnetic connector. Keep the
computer end connected. Seeed notes that this may require several attempts.
The single-connection hard reset and the rapid double-connection bootloader
sequence are different. A green light alone does not establish that a drive
mounted. See the [vendor instructions](https://wiki.seeedstudio.com/sensecap_t1000_e/).

## Return to Meshtastic

Follow the [Meshtastic nRF52 erase and install guide](https://meshtastic.org/docs/getting-started/flashing-firmware/nrf52/nrf52-erase/).
Use its erase utility matching the SoftDevice in `INFO_UF2.TXT`, then install
the official **T1000-E** application UF2. Erase removes device settings. Do not
substitute a UF2 for a different board. The Hopspot flasher links this guide;
third-party firmware is obtained from its own publisher.

## USB contract

The application identifies itself as USB `1209:0001`, manufacturer
`Stay Personal`, product `Personal Hopspot (T1000-E)`, serial
`PERSONAL-RNS-T1000E-HOP`, with its sole WebUSB interface numbered zero.

Both requests are vendor, device-recipient, OUT control transfers with
`wValue=0x5052`, `wIndex=0x4e53`, and no data stage:

| Request | T1000-E reset mode | Purpose |
| --- | --- | --- |
| `0x50` | GPREGRET `0x4e` | Serial DFU used by the existing Hopspot updater |
| `0x55` | GPREGRET `0x57` | Serial DFU plus the stock UF2 recovery drive |

The canonical command constants belong to `prns-core` USB Auto. The firmware
handler rejects malformed signatures and unsupported modes. The browser checks
the exact application identity and interface before sending the request, closes
the device afterward, and excludes concurrent installs. Entering recovery
discards any prepared install and invalidates pending preparation.

## Physical evidence

On 2026-10-01, a T1000-E running Hopspot 0.3.7 at source commit
`bee0d7000a6235696cd714fb1bb4b6b17f74cf9c` was recovered using the vendor's
rapid connector sequence. Its stock bootloader reported
`0.9.1-5-g488711a`, board ID `nRF52840-T1000-E-v1`, and S140 `7.3.0`.
After the matching Meshtastic erase utility and official application UF2 were
installed, a Meshtastic serial protocol query confirmed firmware
`2.7.26.54e0d8d` and hardware model `TRACKER_T1000_E`.

That observation establishes recovery for the older installed firmware using
the stock bootloader. It does not qualify the new software request or startup
shortcut. Release qualification must observe the drive after each new route
and verify the replacement application over its own protocol. Automated
browser tests use simulated USB devices and cannot establish physical reset
or drive enumeration.

## Verification on macOS, 2026-10-01

- `./tools/prns run build.hopspot.t1000e`: recovery UF2 built at application
  base `0x27000`, nRF52840 family `0xADA52840`.
- `cargo test --locked -p prns-flash-manifest`: 79 tests passed.
- `cargo test --locked --manifest-path prns-interfaces/impls/embassy/Cargo.toml --features usb usb_auto::device`:
  five tests passed, including both commands and malformed transfers.
- `cargo test --locked --manifest-path docs/website/Cargo.toml`: 51 tests passed.
- `npm run test:flasher` in `docs/website`: 82 tests passed, including recovery
  without preparation, invalidation, concurrency, cancellation, and failures.
- `npm run test:browser` in `docs/website`: all 36 Chromium tests passed on
  the integrated upstream static website. This includes the recovery UI,
  production Nordic bridge, existing guided installs, and static route
  hydration. Recovery UI accessibility was checked with axe. Before
  integration, four focused tests also passed. The initial browser invocation
  could not launch because its pinned Chromium was absent; subsequent runs
  used that pinned browser after installing it into temporary storage.
- Clippy with warnings denied passed for the manifest, website, USB handler,
  and the T1000-E and T096 target configurations. The shared SoftDevice reset
  callback also passed target Clippy for MeshPocket with
  `mesh-pocket-battery-5000`, Muzi Base Duo with `softdevice-s140-v6`, and
  RAK4631. The first MeshPocket invocation omitted its required battery
  feature and was corrected before compiling that target. Touched Rust files
  were formatted; documentation links and `git diff --check` were clean.

No new firmware was installed on the physical tracker during these checks.
The restored Meshtastic installation remains in place. The new routes are
pending physical qualification and firmware/site release.
