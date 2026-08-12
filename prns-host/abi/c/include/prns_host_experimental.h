#ifndef PRNS_HOST_EXPERIMENTAL_H
#define PRNS_HOST_EXPERIMENTAL_H

/*
 * Experimental extensions to the host contract. Everything here is
 * hand-written, outside the generated schema surface, and free to change
 * shape between releases while a design settles.
 */

#include "prns_host.h"

#if defined(__cplusplus)
extern "C" {
#endif

/*
 * Opens one connected, stream-oriented OS handle and returns it. A
 * non-negative return is a POSIX file descriptor whose ownership transfers
 * to the engine; the engine makes it non-blocking, reads and writes RNS
 * HDLC framing over it, and closes it when the stream ends. A negative
 * return declines this attempt; the engine waits out the configured respawn
 * delay and calls again, so reconnection is simply the callback being asked
 * for a fresh stream.
 *
 * The callback is invoked from engine worker threads, never from the
 * caller's thread. It must be thread-safe, may block while it dials, and
 * together with its context must stay valid until the interface has been
 * detached or the host released.
 *
 * Windows note: this experimental revision is POSIX-only and reports
 * PRNS_STATUS_UNSUPPORTED on other platforms; a settled shape would carry a
 * SOCKET here.
 */
typedef int64_t (*PrnsSuppliedStreamOpenCallback)(void *context);

#define PRNS_INTERFACE_KIND_SUPPLIED_STREAM UINT32_C(20)

/*
 * Attaches an interface whose transport the application supplies, instead of
 * naming an address, device, or command for the engine to open. The name
 * distinguishes concurrent supplied streams on one host and is part of the
 * interface identity, so a caller reattaching after a detach should keep its
 * name. Wire behaviour matches the Pipe family: RNS HDLC framing over the
 * byte stream.
 */
PRNS_HOST_API PrnsStatus prns_host_attach_supplied_stream(
    PrnsHost *host,
    PrnsSuppliedStreamOpenCallback open,
    void *context,
    PrnsStringView name,
    uint64_t respawn_delay_millis,
    PrnsBitrateKind bitrate_kind,
    uint64_t bitrate_bps,
    PrnsIssuedCommand **out_command);

#if defined(__cplusplus)
}
#endif

#endif
