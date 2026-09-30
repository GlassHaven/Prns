use std::io;

use super::{Destination, ReceivedDatagram};
use prns_core::interfaces::wifi_halow::PeerMac;
use prns_core::interfaces::MacAddress;
pub use prns_ffi::ethernet::EtherType;
use prns_ffi::ethernet::{PacketSocket, Reception};
use tokio::io::unix::AsyncFd;

const DISCARD_BURST_LIMIT: usize = 32;

/// One shared normal-data socket on an already configured Linux mesh device.
/// This does not configure the radio or inject 802.11 management frames.
pub struct HaLowSocket(AsyncFd<PacketSocket>);

impl HaLowSocket {
    pub fn bind(interface: &str, protocol: EtherType) -> io::Result<Self> {
        AsyncFd::new(PacketSocket::bind(interface, protocol)?).map(Self)
    }

    /// Success means the kernel accepted the frame, not that any peer received it.
    pub async fn send(&self, destination: Destination, payload: &[u8]) -> io::Result<()> {
        let address = match destination {
            Destination::Broadcast => MacAddress::new([0xff; 6]),
            Destination::Peer(peer) => peer.address(),
        };
        loop {
            let mut ready = self.0.writable().await?;
            match ready.try_io(|socket| socket.get_ref().send(address, payload)) {
                Ok(Err(error)) if error.kind() == io::ErrorKind::Interrupted => continue,
                Ok(result) => return result,
                Err(_) => continue,
            }
        }
    }

    /// The first datagram carries its source identity without consulting a peer table.
    pub async fn receive(&self, buffer: &mut [u8]) -> io::Result<ReceivedDatagram> {
        if buffer.is_empty() {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        let mut discarded = 0;
        loop {
            let mut ready = self.0.readable().await?;
            let received = match ready.try_io(|socket| socket.get_ref().receive(buffer)) {
                Ok(Err(error)) if error.kind() == io::ErrorKind::Interrupted => continue,
                Ok(result) => result?,
                Err(_) => continue,
            };
            if let Reception::Frame { source, length } = received {
                if let Ok(source) = PeerMac::new(source) {
                    return Ok(ReceivedDatagram { source, length });
                }
            }
            discarded += 1;
            if discarded == DISCARD_BURST_LIMIT {
                discarded = 0;
                tokio::task::yield_now().await;
            }
        }
    }
}
