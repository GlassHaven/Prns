use std::future::{poll_fn, Future};
use std::pin::Pin;
use std::task::Poll;
use std::time::Duration;

use personal_rns::routing::links::LinkId;
use personal_rns::wire::{ContextFlag, DestinationType, IfacFlag, PacketType, PropagationType};

use super::*;

const WAIT_BUDGET: Duration = Duration::from_secs(1);

fn advertisement(link: LinkId) -> WirePacketHeader {
    WirePacketHeader {
        ifac_flag: IfacFlag::Open,
        context_flag: ContextFlag::Unset,
        propagation: PropagationType::Broadcast,
        destination_type: DestinationType::Link,
        packet_type: PacketType::Data,
        hops: 0,
        transport_id: None,
        address: link.to_address(),
        context: personal_rns::wire::WireContext::ResourceAdvertisement,
    }
}

fn frame(header: WirePacketHeader) -> Vec<u8> {
    let mut bytes = vec![0; header.wire_len()];
    assert_eq!(header.write(&mut bytes), Ok(bytes.len()));
    bytes
}

async fn pending<F: Future>(mut future: Pin<&mut F>) {
    poll_fn(|cx| {
        assert!(future.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
}

async fn passes(gate: &WireGate, frame: &[u8]) {
    tokio::time::timeout(WAIT_BUDGET, gate.before_send(frame))
        .await
        .unwrap();
}

#[tokio::test(start_paused = true)]
async fn only_the_selected_occurrence_and_complete_header_hold_a_send() {
    let gate = WireGate::new();
    let header = advertisement(LinkId::new([1; 16]));
    let bytes = frame(header);
    passes(&gate, &bytes).await;
    assert!(gate.is_idle());
    gate.arm(header, NonZeroUsize::new(2).unwrap());
    passes(&gate, &[]).await;
    for different in [
        WirePacketHeader {
            address: LinkId::new([2; 16]).to_address(),
            ..header
        },
        WirePacketHeader {
            context: personal_rns::wire::WireContext::Resource,
            ..header
        },
        WirePacketHeader {
            packet_type: PacketType::Proof,
            ..header
        },
    ] {
        passes(&gate, &frame(different)).await;
    }
    passes(&gate, &bytes).await;
    let mut sending = Box::pin(gate.before_send(&bytes));
    pending(sending.as_mut()).await;
    assert_eq!(
        tokio::time::timeout(WAIT_BUDGET, gate.held())
            .await
            .unwrap(),
        header
    );
    gate.release();
    tokio::time::timeout(WAIT_BUDGET, sending).await.unwrap();
    assert!(gate.is_idle());
    passes(&gate, &bytes).await;
}

#[tokio::test(start_paused = true)]
async fn cancelling_a_held_send_releases_ownership_and_allows_rearming() {
    let gate = WireGate::new();
    let header = advertisement(LinkId::new([1; 16]));
    let bytes = frame(header);
    for _ in 0..2 {
        gate.arm(header, NonZeroUsize::MIN);
        let mut sending = Box::pin(gate.before_send(&bytes));
        pending(sending.as_mut()).await;
        assert_eq!(
            tokio::time::timeout(WAIT_BUDGET, gate.held())
                .await
                .unwrap(),
            header
        );
        drop(sending);
        assert!(gate.is_idle());
    }
}

#[tokio::test(start_paused = true)]
async fn held_and_release_notifications_wake_without_advancing_time() {
    let gate = WireGate::new();
    let header = advertisement(LinkId::new([1; 16]));
    let bytes = frame(header);
    gate.arm(header, NonZeroUsize::MIN);
    let started = tokio::time::Instant::now();
    tokio::time::timeout(WAIT_BUDGET, async {
        tokio::join!(
            biased;
            async { assert_eq!(gate.held().await, header); gate.release(); },
            gate.before_send(&bytes),
        );
    })
    .await
    .unwrap();
    assert_eq!(started.elapsed(), Duration::ZERO);
    assert!(gate.is_idle());
}

#[test]
#[should_panic(expected = "one armed wire gate per radio")]
fn an_armed_gate_cannot_be_overwritten() {
    let gate = WireGate::new();
    let header = advertisement(LinkId::new([1; 16]));
    gate.arm(header, NonZeroUsize::MIN);
    gate.arm(header, NonZeroUsize::MIN);
}
