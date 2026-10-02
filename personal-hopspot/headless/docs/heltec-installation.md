# Heltec HT-HD01-V2 application installation

Hopspot runs as an application on the device's existing vendor Linux system.
Choose the exact HT-HD01-V2 model; this guide does not qualify other Heltec
HaLoW products or regional variants.

**Development preview:** the same static MIPS application has passed three-board
HaLoW page transfers and authenticated Remote Control checks on a ThinkNode G4
and two HT-HD01-V2 devices. Persistent installation, signed public downloads and
automatic recovery after radio recreation are not ready. There is no install
button for this appliance yet.

You need an independent Ethernet management connection, authenticated SSH access,
and private configuration/identity backups. Discover the board's management
address and verify its SSH host key; do not assume a shared factory address or
password. Keep the working vendor radio firmware and calibration.

Developers can currently use the repository's `build.hopspot.g4` task to create
the common MIPS development bundle. The G4 name identifies the initial build
recipe; the same executable was tested on these two Heltec boards. Follow the
[temporary application procedure](https://github.com/KenAKAFrosty/Prns/blob/main/personal-hopspot/headless/docs/thinknode-g4.md)
with the Heltec's actual address and device details. Never upload this executable
through LuCI firmware upgrade or `sysupgrade`.

The intended installation flow is a verified download, guided transfer, temporary
health check and a persistent supervised service that preserves identity and
offers rollback. Current tests use RAM-backed `/tmp`; both the application and
temporary identity disappear at reboot. A USB Ethernet adapter supplies network
access, not a qualified USB programming interface.

The [shared HaLoW deployment specification](https://github.com/KenAKAFrosty/Prns/blob/main/personal-hopspot/headless/docs/halow-deployment.md)
describes compatibility, radio/controller setup, storage and recovery work.
The current channel profile is a US lab profile; region and legal power must be
validated for each board. Announcements are explicitly controller-operated.
