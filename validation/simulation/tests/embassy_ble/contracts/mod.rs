mod adapter;
mod tests;

use crate::clock::{ClockLease, CompletionBudget, EmbassyTasks};
use crate::tick;
use adapter::{Node, Runtime};
use personal_rns::engine::{RequestResponseTimeout, SendRequestFailure};
use personal_rns::interfaces::bluetooth_auto::BleAddress;
use personal_rns::interfaces::{ConnectionState, InterfaceId, InterfaceKind};
use personal_rns::routing::links::LinkId;
use personal_rns::runtime::SendError;
use personal_rns::units::{DurationMillis, RttMillis};
use personal_rns::wire::{
    ContextFlag, DestinationType, IfacFlag, PacketType, PropagationType, WireContext,
    WirePacketHeader,
};
use prns_simulation::ble::{BleMediumConfig, BleSimulationEvent, BleWireCapture, VirtualBleLab};
use prns_simulation::{
    ManualMedium, ManualTaskScheduling, ManualTimeDriver, Reachability, TopologyConfig,
    TopologyMutation,
};
use std::num::NonZeroUsize;
use std::time::Duration;

const ADDRESSES: [u8; 2] = [1, 2];
const RESPONSE_TIMEOUT_MS: u64 = 50;
const CAPTURE_CAPACITY: usize = 4096;
const DISCOVERY_BUDGET_MS: u64 = crate::fixture::ADVERTISING_INTERVAL_MS * 3;
type Reply = Result<(Vec<u8>, RttMillis), SendError<SendRequestFailure>>;

#[derive(Debug, PartialEq, Eq)]
struct Report {
    initial: [Reply; 2],
    expired: [TimedReply; 2],
    timeout_elapsed_ms: u64,
    after_expiry: [Reply; 2],
    after_cancellation: [Reply; 2],
    after_reconnect: [Reply; 2],
}

struct Observation {
    report: Report,
    recovery_elapsed_ms: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct TimedReply {
    reply: Reply,
    elapsed_ms: u64,
}

async fn expiring_request(handle: &adapter::Handle, link: LinkId) -> TimedReply {
    let before = embassy_time::Instant::now().as_millis();
    let reply = handle
        .request(
            link,
            &[42],
            RequestResponseTimeout::Exact(DurationMillis(RESPONSE_TIMEOUT_MS)),
        )
        .await;
    TimedReply {
        reply,
        elapsed_ms: embassy_time::Instant::now().as_millis() - before,
    }
}

fn converge(tasks: &mut EmbassyTasks<'_>, lab: &VirtualBleLab, nodes: &[Node; 2]) {
    let expected = ADDRESSES.map(|address| {
        vec![(
            InterfaceId::from_channel_tag(InterfaceKind::BluetoothPeer, &[address; 16]),
            ConnectionState::Connected,
        )]
    });
    let expected = [expected[1].clone(), expected[0].clone()];
    let horizon = tick(tasks.snapshot().tick.get() + DISCOVERY_BUDGET_MS);
    for _ in 0..32 {
        tasks.settle();
        if nodes.each_ref().map(Node::members) == expected {
            assert_eq!(lab.active_connection_count(), 1);
            return;
        }
        assert!(
            tasks.snapshot().tick < horizon,
            "contract discovery deadline: members={:?}, trace={:?}",
            nodes.each_ref().map(Node::members),
            lab.trace().events.iter().rev().take(12).collect::<Vec<_>>()
        );
        tasks.advance_to_next_wake(horizon).unwrap();
    }
    unreachable!("bounded contract discovery");
}

fn establish(tasks: &mut EmbassyTasks<'_>, nodes: &[Node; 2]) -> [LinkId; 2] {
    let handles = nodes.each_ref().map(Node::handle);
    tasks.complete_ready(async move {
        tokio::join!(
            handles[0].announce(ADDRESSES[0]),
            handles[1].announce(ADDRESSES[1])
        );
    });
    let handles = nodes.each_ref().map(Node::handle);
    let links = tasks.complete_ready(async move {
        let (left, right) = tokio::join!(
            handles[0].establish(ADDRESSES[1]),
            handles[1].establish(ADDRESSES[0])
        );
        [left, right]
    });
    assert_ne!(links[0], links[1]);
    links
}

fn exchange(
    tasks: &mut EmbassyTasks<'_>,
    nodes: &[Node; 2],
    links: [LinkId; 2],
    marker: u8,
) -> [Reply; 2] {
    let handles = nodes.each_ref().map(Node::handle);
    let replies = tasks.complete_ready(async move {
        let data = [marker; 256];
        let (left, right) = tokio::join!(
            handles[0].request(links[0], &data, RequestResponseTimeout::LinkDefault),
            handles[1].request(links[1], &data, RequestResponseTimeout::LinkDefault),
        );
        [left, right]
    });
    for node in nodes {
        node.drain_fixture_diagnostics();
    }
    replies
}

fn run(runtimes: [Runtime; 2], scheduling: ManualTaskScheduling) -> Observation {
    let clock = ClockLease::acquire();
    let capture = BleWireCapture::new(NonZeroUsize::new(CAPTURE_CAPACITY).unwrap());
    let lab = VirtualBleLab::with_wire_capture(
        BleMediumConfig::new(
            TopologyConfig::Explicit {
                max_neighbors: NonZeroUsize::MIN,
            },
            2,
            4,
            4,
            CAPTURE_CAPACITY,
        )
        .unwrap(),
        capture.clone(),
    );
    let mut driver =
        ManualTimeDriver::new(ManualMedium::Ble(lab.clone()), Duration::from_millis(1)).unwrap();
    let mut tasks = EmbassyTasks::with_limits(
        &mut driver,
        clock,
        NonZeroUsize::new(8).unwrap(),
        NonZeroUsize::new(256).unwrap(),
        scheduling,
    );
    let nodes = std::array::from_fn(|index| {
        Node::start(runtimes[index], &mut tasks, &lab, ADDRESSES[index])
    });
    let addresses = ADDRESSES.map(|address| BleAddress::new([address; 6]));
    assert_eq!(
        lab.set_reachability(addresses[0], addresses[1], Reachability::Reachable),
        Ok(TopologyMutation::Applied)
    );
    converge(&mut tasks, &lab, &nodes);
    let links = establish(&mut tasks, &nodes);
    let initial = exchange(&mut tasks, &nodes, links, 41);

    for (index, link) in links.iter().enumerate() {
        nodes[index ^ 1]
            .wire()
            .lose_after(response_header(*link), 0, NonZeroUsize::MIN);
    }
    let handles = nodes.each_ref().map(Node::handle);
    let before = tasks.snapshot().tick.get();
    let expired = tasks.complete_with_budget(
        CompletionBudget {
            deadline: tick(before + RESPONSE_TIMEOUT_MS),
            polls_per_tick: NonZeroUsize::new(256).unwrap(),
        },
        async move {
            let (left, right) = tokio::join!(
                expiring_request(&handles[0], links[0]),
                expiring_request(&handles[1], links[1])
            );
            [left, right]
        },
    );
    let timeout_elapsed_ms = tasks.snapshot().tick.get() - before;
    for node in &nodes {
        assert_eq!(node.wire().stop_loss(), 1);
    }
    let after_expiry = exchange(&mut tasks, &nodes, links, 43);
    for _ in 0..crate::node::REQUEST_CAPACITY {
        for (index, link) in links.iter().copied().enumerate() {
            let gate = nodes[index ^ 1].wire().clone();
            gate.lose_after(response_header(link), 0, NonZeroUsize::MIN);
            let handle = nodes[index].handle();
            tasks.complete_ready(async move {
                tokio::select! {
                    biased;
                    reply = handle.request(link, &[42], RequestResponseTimeout::LinkDefault) => unreachable!("dropped response completed: {reply:?}"),
                    _ = gate.first_loss() => {},
                }
            });
            assert_eq!(nodes[index ^ 1].wire().stop_loss(), 1);
        }
    }
    let after_cancellation = exchange(&mut tasks, &nodes, links, 45);
    let boundary = capture.snapshot();
    let recovery_start = tasks.snapshot().tick.get();
    assert_eq!(
        lab.set_reachability(addresses[0], addresses[1], Reachability::Isolated),
        Ok(TopologyMutation::Applied)
    );
    tasks.settle();
    assert_eq!(lab.active_connection_count(), 0);
    assert_eq!(nodes.each_ref().map(Node::members), [vec![], vec![]]);
    assert_eq!(capture.snapshot(), boundary);
    assert_eq!(
        lab.set_reachability(addresses[0], addresses[1], Reachability::Reachable),
        Ok(TopologyMutation::Applied)
    );
    converge(&mut tasks, &lab, &nodes);
    let recovery_elapsed_ms = tasks.snapshot().tick.get() - recovery_start;
    let fresh = establish(&mut tasks, &nodes);
    for link in fresh {
        assert!(!links.contains(&link));
    }
    let after_reconnect = exchange(&mut tasks, &nodes, fresh, 44);
    drop(nodes);
    drop(tasks);
    assert_eq!(lab.active_connection_count(), 0);
    let wire = capture.snapshot();
    assert_eq!(wire.discarded_values, 0);
    let (old, new) = wire.values.split_at(boundary.values.len());
    assert!(!old.is_empty() && !new.is_empty());
    let mut retired = Vec::new();
    for value in old {
        if !retired.contains(&value.connection) {
            retired.push(value.connection);
        }
    }
    assert!(
        new.iter().all(|value| !retired.contains(&value.connection)),
        "retired connection identities cannot carry recovery traffic: {runtimes:?}"
    );
    let trace = lab.trace();
    assert_eq!(trace.discarded_events, 0);
    let mut attached = Vec::new();
    let mut detached = Vec::new();
    for event in trace.events {
        match event {
            BleSimulationEvent::RadioAttached { radio } => attached.push(radio),
            BleSimulationEvent::RadioDetached { radio } => detached.push(radio),
            _ => {}
        }
    }
    attached.sort();
    detached.sort();
    assert_eq!(attached.len(), 2);
    assert_eq!(attached, detached);
    Observation {
        report: Report {
            initial,
            expired,
            timeout_elapsed_ms,
            after_expiry,
            after_cancellation,
            after_reconnect,
        },
        recovery_elapsed_ms,
    }
}

fn response_header(link: LinkId) -> WirePacketHeader {
    WirePacketHeader {
        ifac_flag: IfacFlag::Open,
        context_flag: ContextFlag::Unset,
        propagation: PropagationType::Broadcast,
        destination_type: DestinationType::Link,
        packet_type: PacketType::Data,
        hops: 0,
        transport_id: None,
        address: link.to_address(),
        context: WireContext::Response,
    }
}
