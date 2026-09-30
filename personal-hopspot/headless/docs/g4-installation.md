# ThinkNode G4 application installation

Hopspot runs as a Linux application on the G4's existing operating system. The
installation workflow starts here in the web flasher, with a downloadable
application and guided setup rather than a replacement firmware image.

**Development preview:** TCP and an experimental native HaLoW interface have
served a complete Hopspot page on a G4. A signed public download and persistent
installer are not ready yet. There is no public install button for this board
at this stage.

## What you will need

- A ThinkNode G4 with its working vendor system and a reachable management page.
- An Ethernet connection to the device and authenticated SSH access.
- A private backup of device configuration and Hopspot identity before updates.

The intended flow is to download a verified application bundle from this page,
transfer it to the device, run a temporary health check, and then activate a
service that survives reboot. Updates must preserve identity and provide a way
back to the previous working application.

## Try the current development application

Developers can build the bundle using the repository's `build.hopspot.g4` task.
It contains the application, checksums, build information, and `INSTALL.md` with
the tested temporary deployment procedure. Follow that procedure to run the TCP
host in RAM and verify its Hopspot page from a second machine.

[Open the build and temporary installation guide](https://github.com/KenAKAFrosty/Prns/blob/main/personal-hopspot/headless/docs/thinknode-g4.md).

The current bundle is unsigned development output. It is **not** an image for
LuCI firmware upgrade or `sysupgrade`. A temporary installation in `/tmp` loses
its application and identity at reboot; it does not set up a persistent service.

## Browser connection options

The present G4 management path is Ethernet. A USB-to-Ethernet adapter provides
network access, not a USB programming connection to the G4. A browser upload
workflow would need a compatible authenticated endpoint on the device or a local
helper. Those paths are being evaluated; USB serial recovery has not been
qualified for this board.

HaLoW is opt-in in the development bundle and requires a separately configured
radio; its first end-to-end check used the vendor AP/station connection. Mesh,
field reliability, Wi-Fi discovery, and browser-node access need further qualification.
See the repository's headless README for the experimental radio options.
