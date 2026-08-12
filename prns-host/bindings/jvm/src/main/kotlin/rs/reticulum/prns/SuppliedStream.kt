package rs.reticulum.prns

/**
 * Opens one connected, stream-oriented file descriptor on behalf of the
 * engine — for example a socket this application dialed itself, protected
 * through a `VpnService`, or received from another component.
 *
 * A non-negative return hands the descriptor to the engine, which makes it
 * non-blocking, carries RNS HDLC framing over it, and closes it when the
 * stream ends — so return a descriptor the engine may own, such as one from
 * `ParcelFileDescriptor.detachFd()`. A negative return (or a thrown
 * exception) declines this attempt; the engine waits out the respawn delay
 * and asks again, which is also how reconnection works.
 *
 * Called from engine worker threads, never the caller's thread; it may block
 * while it dials. The [Host] keeps the opener referenced until the host is
 * closed.
 */
fun interface SuppliedStreamOpener {
    fun open(): Long
}

internal const val SUPPLIED_STREAM_DECLINED: Long = -1L
