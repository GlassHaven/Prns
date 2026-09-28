use super::*;
use crate::scenario::POLL_BUDGET;
use prns_simulation::ble::BleDataCounters;
use prns_simulation::{ManualTaskPoll, ManualTaskScheduling, SimulationSeed};

#[derive(Clone, Copy, Debug)]
enum CutBoundary {
    Queued,
    Consumed,
}

#[derive(Clone, Copy, Debug)]
enum Direction {
    TowardBle,
    FromBle,
}

impl Direction {
    fn requester(self) -> usize {
        match self {
            Self::TowardBle => 0,
            Self::FromBle => BLE_PEER,
        }
    }

    fn responder(self) -> usize {
        match self {
            Self::TowardBle => BLE_PEER,
            Self::FromBle => 0,
        }
    }

    fn sender(self) -> usize {
        match self {
            Self::TowardBle => BRIDGE,
            Self::FromBle => BLE_PEER,
        }
    }

    fn receiver(self) -> usize {
        match self {
            Self::TowardBle => BLE_PEER,
            Self::FromBle => BRIDGE,
        }
    }

    fn counters(self, ble: &VirtualBleLab) -> BleDataCounters {
        let snapshots = ble.data_snapshots();
        let [connection] = snapshots.as_slice() else {
            unreachable!("exactly one active BLE connection");
        };
        let sender = BleAddress::new([self.sender() as u8; 6]);
        let receiver = BleAddress::new([self.receiver() as u8; 6]);
        let counters = if connection.dialer == sender {
            assert_eq!(connection.listener, receiver);
            connection.dialer_to_listener
        } else {
            assert_eq!((connection.dialer, connection.listener), (receiver, sender));
            connection.listener_to_dialer
        };
        assert!(!counters.saturated);
        counters
    }
}

fn stop_partial(
    runner: &mut ManualTaskRunner<'_, Completion>,
    ble: &VirtualBleLab,
    nodes: &[fixture::Node],
    direction: Direction,
    boundary: CutBoundary,
    before: BleDataCounters,
) {
    let clock = runner
        .snapshot()
        .unwrap_or_else(|error| unreachable!("fragment clock: {error}"));
    for _ in 0..POLL_BUDGET {
        let Some(ManualTaskPoll::Pending { task }) = runner.poll_next().ok() else {
            unreachable!("request must remain pending at the partial-frame boundary");
        };
        let current = direction.counters(ble);
        if current.fragments_queued == before.fragments_queued {
            continue;
        }
        assert_eq!(current.sends_started, before.sends_started + 1);
        assert_eq!(current.sends_completed, before.sends_completed);
        assert_eq!(current.frames_reassembled, before.frames_reassembled);
        assert_eq!(current.fragments_queued, before.fragments_queued + 2);
        let expected_actor = match boundary {
            CutBoundary::Queued => {
                assert_eq!(current.values_consumed, before.values_consumed);
                direction.sender()
            }
            CutBoundary::Consumed => {
                if current.values_consumed == before.values_consumed {
                    continue;
                }
                assert_eq!(current.values_consumed, before.values_consumed + 2);
                direction.receiver()
            }
        };
        assert_eq!(task, nodes[expected_actor].task);
        assert_eq!(runner.snapshot().ok(), Some(clock));
        return;
    }
    unreachable!("request must reach the selected fragment boundary within the poll budget");
}

fn exercise(
    direction: Direction,
    boundary: CutBoundary,
    profile: Profile,
    scheduling: ManualTaskScheduling,
) {
    with_bridge(
        profile,
        scheduling,
        2,
        |runner, frames, ble, _, nodes, frame_id| {
            converge(runner, ble, nodes);
            routes(runner, nodes, frame_id);
            let crossing = link(runner, nodes, direction.requester(), direction.responder());
            let local = link(runner, nodes, 0, BRIDGE);
            let tasks: Vec<_> = nodes.iter().map(|node| node.task).collect();
            for cycle in 0..CYCLES {
                let before = direction.counters(ble);
                let bytes = vec![0x30 + cycle as u8; 256];
                let control = request(
                    runner,
                    direction.requester(),
                    nodes[direction.requester()].control.handle.clone(),
                    crossing,
                    bytes.clone(),
                );
                stop_partial(runner, ble, nodes, direction, boundary, before);
                assert_eq!(
                    settle(runner),
                    [(
                        control,
                        Completion::Response {
                            node: direction.requester(),
                            bytes
                        }
                    )]
                );

                let before = direction.counters(ble);
                let started = frames.now();
                let pending = lost_requests(runner, nodes, &[(direction.requester(), crossing)]);
                stop_partial(runner, ble, nodes, direction, boundary, before);
                assert_eq!(
                    ble.set_reachability(
                        BleAddress::new([BRIDGE as u8; 6]),
                        BleAddress::new([BLE_PEER as u8; 6]),
                        Reachability::Isolated
                    ),
                    Ok(TopologyMutation::Applied)
                );
                assert!(settle(runner).is_empty());
                assert!(ble.data_snapshots().is_empty());
                assert_eq!(ble.active_connection_count(), 0);
                echo(runner, nodes, 0, local, 0x40 + cycle as u8);
                expire(runner, frames, ble, started, pending);
                assert_eq!(runner.task_count(), 3);
                assert_eq!(
                    ble.set_reachability(
                        BleAddress::new([BRIDGE as u8; 6]),
                        BleAddress::new([BLE_PEER as u8; 6]),
                        Reachability::Reachable
                    ),
                    Ok(TopologyMutation::Applied)
                );
                converge(runner, ble, nodes);
                routes(runner, nodes, frame_id);
                echo(
                    runner,
                    nodes,
                    direction.requester(),
                    crossing,
                    0x60 + cycle as u8,
                );
                echo(runner, nodes, 0, local, 0x70 + cycle as u8);
                assert_eq!(
                    nodes.iter().map(|node| node.task).collect::<Vec<_>>(),
                    tasks
                );
                traffic::clocks(runner, nodes, [tick(0); 3], frames, ble);
            }
        },
    );
}

fn matrix(boundary: CutBoundary) {
    for direction in [Direction::TowardBle, Direction::FromBle] {
        for profile in [Profile::AppleBridge, Profile::BluezBridge] {
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
                exercise(direction, boundary, profile, scheduling);
            }
        }
    }
}

#[test]
fn mixed_fragmented_request_cut_after_queueing_partial_frame() {
    matrix(CutBoundary::Queued);
}

#[test]
fn mixed_fragmented_request_cut_after_consuming_partial_frame() {
    matrix(CutBoundary::Consumed);
}
