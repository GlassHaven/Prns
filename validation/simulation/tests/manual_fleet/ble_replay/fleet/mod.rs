use super::*;
use personal_rns::routing::links::LinkId;
use personal_rns::wire::DestinationHash;
use prns_simulation::{ManualTaskScheduling, SimulationSeed};
use std::collections::BTreeSet;

mod traffic;

const NODES: usize = 8;
const PAIRS: usize = NODES / 2;
const TRACE_EVENTS: usize = 262_144;
const CAPTURE_VALUES: usize = 16_384;

#[derive(Clone, Copy)]
struct Inputs {
    scheduling: ManualTaskScheduling,
    host_seed: u8,
    payload_marker: u8,
}

#[derive(Debug, PartialEq, Eq)]
struct FleetTranscript {
    discovery: BleTraceSnapshot,
    wire: BleWireSnapshot,
    responses: Vec<Vec<(usize, Vec<u8>)>>,
    boundaries: Vec<usize>,
}

fn address(node: usize) -> BleAddress {
    BleAddress::new([node as u8; 6])
}

fn expected_peer(node: usize) -> Vec<(InterfaceId, ConnectionState)> {
    vec![(
        InterfaceId::from_channel_tag(InterfaceKind::BluetoothPeer, &[(node ^ 1) as u8; 16]),
        ConnectionState::Connected,
    )]
}

fn run(inputs: Inputs) -> FleetTranscript {
    let capture = BleWireCapture::new(nonzero(CAPTURE_VALUES));
    let lab = VirtualBleLab::with_wire_capture(
        BleMediumConfig::new(
            TopologyConfig::Explicit {
                max_neighbors: nonzero(1),
            },
            NODES,
            4,
            NODES,
            TRACE_EVENTS,
        )
        .unwrap_or_else(|error| unreachable!("paired lab: {error}")),
        capture.clone(),
    );
    let mut driver =
        ManualTimeDriver::new(ManualMedium::Ble(lab.clone()), Duration::from_millis(1))
            .unwrap_or_else(|error| unreachable!("clock: {error}"));
    let mut runner = ManualTaskRunner::new_with_scheduling(
        &mut driver,
        nonzero(NODES + PAIRS),
        inputs.scheduling,
    );
    let nodes: Vec<_> = (0..NODES)
        .map(|index| {
            fixture::start_node(
                &mut runner,
                &lab,
                index,
                inputs.host_seed,
                selection::FirstEvent::ALL[index % selection::FirstEvent::ALL.len()],
                if index.is_multiple_of(2) {
                    personal_rns::runtime::InterfaceEventSource::Message
                } else {
                    personal_rns::runtime::InterfaceEventSource::Completion
                },
            )
        })
        .collect();
    let destinations: Vec<_> = (0..NODES)
        .map(|node| {
            destination(node)
                .destination_hash()
                .unwrap_or_else(|error| unreachable!("destination: {error:?}"))
        })
        .collect();
    for node in (0..NODES).step_by(2) {
        assert_eq!(
            lab.set_reachability(address(node), address(node + 1), Reachability::Reachable),
            Ok(TopologyMutation::Applied)
        );
    }
    ble::advance_until(&mut runner, || {
        lab.active_connection_count() == PAIRS
            && nodes
                .iter()
                .enumerate()
                .all(|(node, fixture::LiveNode { control, .. })| {
                    ble::member_inventory(&control.handle) == expected_peer(node)
                })
    });
    for (node, fixture::LiveNode { control, .. }) in nodes.iter().enumerate() {
        announce(control, destinations[node]);
    }
    ble::advance_until(&mut runner, || {
        nodes
            .iter()
            .enumerate()
            .all(|(node, fixture::LiveNode { heard, .. })| {
                *heard.borrow() == [destinations[node ^ 1]]
            })
    });
    let mut links = traffic::establish(&mut runner, &nodes, &destinations, (0..NODES).step_by(2));
    let mut responses = vec![traffic::exchange(
        &mut runner,
        &nodes,
        &links,
        inputs.payload_marker,
    )];
    let before = capture.snapshot();
    let mut boundaries = vec![before.values.len()];
    let old_connections = connection_pairs(&before);
    assert_eq!(old_connections.len(), PAIRS);

    assert_eq!(
        lab.set_reachability(address(0), address(1), Reachability::Isolated),
        Ok(TopologyMutation::Applied)
    );
    assert_eq!(lab.active_connection_count(), PAIRS - 1);
    assert!(settle(&mut runner).is_empty());
    assert_eq!(capture.snapshot(), before);
    for (node, fixture::LiveNode { control, .. }) in nodes.iter().enumerate() {
        assert_eq!(
            ble::member_inventory(&control.handle),
            if node < 2 {
                vec![]
            } else {
                expected_peer(node)
            }
        );
    }
    assert!(links.remove(&0).is_some());
    responses.push(traffic::exchange(
        &mut runner,
        &nodes,
        &links,
        inputs.payload_marker,
    ));
    let isolated = capture.snapshot();
    for value in &isolated.values[boundaries[0]..] {
        assert_ne!(value.from, address(0));
        assert_ne!(value.from, address(1));
        assert_eq!(old_connections.get(&pair(value)), Some(&value.connection));
    }
    boundaries.push(isolated.values.len());
    assert_eq!(
        lab.set_reachability(address(0), address(1), Reachability::Reachable),
        Ok(TopologyMutation::Applied)
    );
    ble::advance_until(&mut runner, || {
        lab.active_connection_count() == PAIRS
            && nodes
                .iter()
                .enumerate()
                .all(|(node, fixture::LiveNode { control, .. })| {
                    ble::member_inventory(&control.handle) == expected_peer(node)
                })
    });
    links.extend(traffic::establish(&mut runner, &nodes, &destinations, [0]));
    responses.push(traffic::exchange(
        &mut runner,
        &nodes,
        &links,
        inputs.payload_marker,
    ));
    let recovered = capture.snapshot();
    let after = BleWireSnapshot {
        discarded_values: 0,
        values: recovered.values[boundaries[1]..].to_vec(),
    };
    let new_connections = connection_pairs(&after);
    assert_eq!(new_connections.len(), PAIRS);
    for (addresses, connection) in &new_connections {
        if *addresses == (address(0), address(1)) {
            assert_ne!(old_connections.get(addresses), Some(connection));
        } else {
            assert_eq!(old_connections.get(addresses), Some(connection));
        }
    }
    boundaries.push(recovered.values.len());

    let expected: BTreeMap<_, _> = nodes
        .into_iter()
        .enumerate()
        .map(|(node, fixture::LiveNode { task, control, .. })| {
            assert_eq!(control.shutdown.send(()), Ok(()));
            (
                task,
                Completion::Stopped {
                    node,
                    result: Ok(()),
                },
            )
        })
        .collect();
    assert_eq!(
        settle(&mut runner).into_iter().collect::<BTreeMap<_, _>>(),
        expected
    );
    assert_eq!(runner.task_count(), 0);
    assert_eq!(lab.active_connection_count(), 0);
    let discovery = lab.trace();
    assert_eq!(discovery.discarded_events, 0);
    let mut attached = Vec::new();
    let mut detached = Vec::new();
    for event in &discovery.events {
        match event {
            BleSimulationEvent::RadioAttached { radio } => attached.push(*radio),
            BleSimulationEvent::RadioDetached { radio } => detached.push(*radio),
            BleSimulationEvent::ObservationDropped { .. } => unreachable!("observation loss"),
            _ => {}
        }
    }
    attached.sort();
    detached.sort();
    assert_eq!(attached.len(), NODES);
    assert_eq!(attached, detached);
    let wire = capture.snapshot();
    assert_eq!(wire.discarded_values, 0);
    FleetTranscript {
        discovery,
        wire,
        responses,
        boundaries,
    }
}

fn pair(value: &prns_simulation::ble::BleWireValue) -> (BleAddress, BleAddress) {
    (value.from.min(value.to), value.from.max(value.to))
}

fn connection_pairs(
    snapshot: &BleWireSnapshot,
) -> BTreeMap<(BleAddress, BleAddress), prns_simulation::ble::BleWireConnectionId> {
    let mut connections = BTreeMap::new();
    for value in &snapshot.values {
        if let Some(previous) = connections.insert(pair(value), value.connection) {
            assert_eq!(previous, value.connection);
        }
    }
    let addresses: BTreeSet<_> = connections.keys().copied().collect();
    assert_eq!(
        addresses,
        (0..NODES)
            .step_by(2)
            .map(|node| (address(node), address(node + 1)))
            .collect()
    );
    let mut distinct = Vec::new();
    for connection in connections.values() {
        assert!(!distinct.contains(connection));
        distinct.push(*connection);
    }
    connections
}

fn assert_replay(actual: &FleetTranscript, expected: &FleetTranscript) {
    for (index, (actual, expected)) in actual
        .wire
        .values
        .iter()
        .zip(&expected.wire.values)
        .enumerate()
    {
        assert_eq!(actual, expected, "wire value {index}");
    }
    for (index, (actual, expected)) in actual
        .discovery
        .events
        .iter()
        .zip(&expected.discovery.events)
        .enumerate()
    {
        assert_eq!(actual, expected, "discovery event {index}");
    }
    assert_eq!(actual, expected);
}

#[test]
fn concurrent_pairs_replay_recovery_under_seeded_actor_orders() {
    for scheduling in [
        ManualTaskScheduling::Cyclic,
        ManualTaskScheduling::Seeded {
            seed: SimulationSeed::new(0),
        },
        ManualTaskScheduling::Seeded {
            seed: SimulationSeed::new(7),
        },
        ManualTaskScheduling::Seeded {
            seed: SimulationSeed::new(u64::MAX),
        },
    ] {
        let inputs = Inputs {
            scheduling,
            host_seed: 11,
            payload_marker: 42,
        };
        let expected = run(inputs);
        for _ in 0..3 {
            assert_replay(&run(inputs), &expected);
        }
    }
}

#[test]
fn fleet_replay_detects_changed_protocol_inputs() {
    let inputs = Inputs {
        scheduling: ManualTaskScheduling::Seeded {
            seed: SimulationSeed::new(7),
        },
        host_seed: 11,
        payload_marker: 42,
    };
    let baseline = run(inputs);
    let seed = run(Inputs {
        host_seed: 21,
        ..inputs
    });
    assert_eq!(baseline.responses, seed.responses);
    assert_ne!(baseline.wire, seed.wire);
    let payload = run(Inputs {
        payload_marker: 43,
        ..inputs
    });
    assert_ne!(baseline.responses, payload.responses);
    assert_ne!(baseline.wire, payload.wire);
}
