use std::fs::File;
use std::future::Future;
use std::io::{self, Read, Write};
use std::os::fd::OwnedFd;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{ready, Context, Poll};
use std::time::Duration;

use personal_rns::interfaces::pipe as pipe_contract;
use personal_rns::interfaces::{BitrateBps, ConfiguredInterfacePolicy};
use personal_rns::pipe::{PipeInterface, PipeRespawnDelay};
use prns_host::Bitrate;
use tokio::io::unix::AsyncFd;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// The application held the stream and would not (or could not) open one now.
/// The engine treats this like a failed connect: it waits out the respawn
/// delay and asks again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SuppliedStreamDeclined {
    pub code: i64,
}

/// Opens one connected, non-blocking, stream-oriented descriptor on demand.
/// Ownership of the descriptor transfers to the engine, which closes it when
/// the stream ends. Invoked from a blocking-friendly thread, so the opener may
/// take its time (dial, protect, hand over). Re-invoked for every reconnect.
pub type SuppliedStreamOpen =
    Arc<dyn Fn() -> Result<OwnedFd, SuppliedStreamDeclined> + Send + Sync>;

pub struct SuppliedStreamAttach {
    /// Distinguishes concurrent supplied streams on one host; part of the
    /// interface identity, so a reconnecting opener must keep its name.
    pub name: String,
    pub open: SuppliedStreamOpen,
    pub respawn_delay: Duration,
    pub bitrate: Bitrate,
}

const CHANNEL_NAMESPACE: &[u8] = b"supplied-stream:";

pub(crate) type OpenFuture = Pin<Box<dyn Future<Output = io::Result<FdStream>> + Send>>;

pub(crate) fn supplied_stream_interface(
    attach: &SuppliedStreamAttach,
    bitrate: BitrateBps,
) -> PipeInterface<impl FnMut() -> OpenFuture> {
    let mut channel_tag = CHANNEL_NAMESPACE.to_vec();
    channel_tag.extend_from_slice(attach.name.as_bytes());
    PipeInterface::with_policy(
        opener(Arc::clone(&attach.open)),
        PipeRespawnDelay::new(attach.respawn_delay),
        pipe_contract::configured_policy(ConfiguredInterfacePolicy {
            bitrate: Some(bitrate),
            ..ConfiguredInterfacePolicy::default()
        }),
        &channel_tag,
    )
}

fn opener(open: SuppliedStreamOpen) -> impl FnMut() -> OpenFuture {
    move || {
        let open = Arc::clone(&open);
        Box::pin(async move {
            let opened = tokio::task::spawn_blocking(move || (open)())
                .await
                .map_err(|join_error| io::Error::other(join_error.to_string()))?;
            let fd = opened.map_err(|declined| {
                io::Error::new(
                    io::ErrorKind::ConnectionRefused,
                    format!("the application declined to supply a stream (code {})", declined.code),
                )
            })?;
            FdStream::supplied(fd)
        }) as OpenFuture
    }
}

/// A connected descriptor the application handed over, driven by the reactor.
/// The descriptor must already be non-blocking; reads and writes go through
/// plain read(2)/write(2), so any pollable stream descriptor works.
pub(crate) struct FdStream {
    fd: AsyncFd<File>,
}

impl FdStream {
    fn supplied(fd: OwnedFd) -> io::Result<Self> {
        Ok(Self {
            fd: AsyncFd::new(File::from(fd))?,
        })
    }
}

impl AsyncRead for FdStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let stream = self.get_mut();
        loop {
            let mut guard = ready!(stream.fd.poll_read_ready(cx))?;
            let unfilled = buf.initialize_unfilled();
            match guard.try_io(|inner| {
                let mut file = inner.get_ref();
                file.read(unfilled)
            }) {
                Ok(Ok(read)) => {
                    buf.advance(read);
                    return Poll::Ready(Ok(()));
                }
                Ok(Err(error)) => return Poll::Ready(Err(error)),
                Err(_would_block) => {}
            }
        }
    }
}

impl AsyncWrite for FdStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let stream = self.get_mut();
        loop {
            let mut guard = ready!(stream.fd.poll_write_ready(cx))?;
            match guard.try_io(|inner| {
                let mut file = inner.get_ref();
                file.write(buf)
            }) {
                Ok(result) => return Poll::Ready(result),
                Err(_would_block) => {}
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    /// Dropping the stream closes the descriptor; a half-close is not
    /// expressible through a borrowed File, and the framing loop only ever
    /// tears the whole stream down.
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
