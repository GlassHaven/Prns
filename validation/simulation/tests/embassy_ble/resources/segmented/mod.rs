use super::*;
use crate::response_trace::{ResponseEvent, ResponseTrace};
use personal_rns::engine::{
    CommandId, DeliveryEvidence, LinkClosedReason, PacketReceiptDelivered, PrnsCommand,
    SendRequest, SendRequestData,
};
use personal_rns::interfaces::bluetooth_auto::BleAddress;
use prns_simulation::{Reachability, TopologyMutation};

mod buffered_interruption;
mod culled;
mod interrupted;
mod stalled;
mod storage;
use storage::{SegmentedStorage, TRANSFER_WINDOW_BYTES};

const _: () = assert!(TRANSFER_BYTES > 2 * TRANSFER_WINDOW_BYTES);
const EMBASSY_RADIO: BleAddress = BleAddress::new([EMBASSY_ADDRESS; 6]);
const TOKIO_RADIO: BleAddress = BleAddress::new([TOKIO_ADDRESS; 6]);
const INTERRUPTION_BUDGET_MS: u64 = 20_000;
const INTERRUPTION_TRACE_CAPACITY: usize = 131_072;
const ORDINARY_RECEIPTS: usize = crate::node::REQUEST_CAPACITY;

enum Requester {
    Embassy,
    Tokio,
}

enum AdmissionCase {
    RefusedAtOffer,
    ThreeSegments,
}

fn journaled_request(
    tasks: &mut EmbassyTasks<'_>,
    handle: &impl PrnsNodeApi,
    trace: ResponseTrace,
    link: LinkId,
    case: &AdmissionCase,
) -> (CommandId, Settlement) {
    assert!(trace.is_empty());
    let started = tasks.snapshot().tick;
    let command = handle
        .issue(PrnsCommand::SendRequest(SendRequest {
            link_id: link,
            path_hash: RequestPathHash::of(files::FILE_PATH),
            data: SendRequestData::from_slice(&(TRANSFER_BYTES as u16).to_be_bytes()).unwrap(),
            response_timeout: RequestResponseTimeout::LinkDefault,
            maximum_response_bytes: match case {
                AdmissionCase::RefusedAtOffer => ByteLimit::Maximum(0),
                AdmissionCase::ThreeSegments => ByteLimit::Unlimited,
            },
        }))
        .unwrap();
    let events = complete(tasks, async move { trace.completed().await });
    let elapsed = RttMillis::new(tasks.snapshot().tick.get() - started.get());
    let result = match case {
        AdmissionCase::RefusedAtOffer => Err(SendRequestFailure::ResponseTooLarge),
        AdmissionCase::ThreeSegments => Ok(PacketReceiptDelivered {
            rtt: elapsed,
            evidence: DeliveryEvidence::Response,
        }),
    };
    let expected_terminal = ResponseEvent::Settled { command, result };
    match case {
        AdmissionCase::RefusedAtOffer => assert_eq!(events, [expected_terminal]),
        AdmissionCase::ThreeSegments => {
            assert_eq!(
                events.len(),
                4,
                "three segments and exactly one settlement: {events:?}"
            );
            assert_eq!(events.last(), Some(&expected_terminal));
            let mut bytes = Vec::new();
            let mut positions = Vec::new();
            for event in &events {
                if let ResponseEvent::Segment {
                    link,
                    request,
                    index,
                    total,
                    bytes: part,
                } = event
                {
                    assert!(part.len() <= TRANSFER_WINDOW_BYTES);
                    positions.push((*link, *request, *index, *total));
                    bytes.extend_from_slice(part);
                }
            }
            assert_eq!(positions.len(), 3);
            let request = positions[0].1;
            assert_eq!(
                positions,
                [
                    (link, request, 1, 3),
                    (link, request, 2, 3),
                    (link, request, 3, 3)
                ]
            );
            assert_eq!(bytes, files::FILE_BYTES[..TRANSFER_BYTES]);
        }
    }
    (command, Settlement::SendRequest(result))
}

fn scenario(
    embedded_endpoint: Endpoint,
    desktop_endpoint: Endpoint,
    trace_capacity: usize,
    run: impl FnOnce(
        &mut EmbassyTasks<'_>,
        &VirtualBleLab,
        &ResourceNode,
        &tokio_node::TokioNode,
        [LinkId; 2],
    ),
) {
    scenario_with_receipts::<ORDINARY_RECEIPTS>(
        embedded_endpoint,
        desktop_endpoint,
        trace_capacity,
        run,
    );
}

fn scenario_with_receipts<const RECEIPTS: usize>(
    embedded_endpoint: Endpoint,
    desktop_endpoint: Endpoint,
    trace_capacity: usize,
    run: impl FnOnce(
        &mut EmbassyTasks<'_>,
        &VirtualBleLab,
        &ResourceNode,
        &tokio_node::TokioNode,
        [LinkId; 2],
    ),
) {
    let clock = ClockLease::acquire();
    let lab = VirtualBleLab::new(
        BleMediumConfig::new(
            TopologyConfig::Explicit {
                max_neighbors: NonZeroUsize::MIN,
            },
            2,
            4,
            4,
            trace_capacity,
        )
        .unwrap(),
    );
    let mut driver =
        ManualTimeDriver::new(ManualMedium::Ble(lab.clone()), Duration::from_millis(1)).unwrap();
    let mut tasks = EmbassyTasks::new(&mut driver, clock);
    let destination =
        |address| destination_with_limit(address, ByteLimit::Maximum(REQUEST_BYTES as u64));
    let embedded = ResourceNode::with_storage(
        &mut tasks,
        &lab,
        EMBASSY_ADDRESS,
        embedded_endpoint,
        [destination(EMBASSY_ADDRESS)],
        personal_rns::request_endpoints![Echo, files::FileReply],
        SegmentedStorage::<RECEIPTS>,
    );
    let mut desktop = tokio_node::with_storage(
        &mut tasks,
        &lab,
        TOKIO_ADDRESS,
        desktop_endpoint,
        [destination(TOKIO_ADDRESS)],
        personal_rns::request_endpoints![Echo, files::FileReply],
        SegmentedStorage::<RECEIPTS>,
    );
    assert_eq!(
        lab.set_reachability(EMBASSY_RADIO, TOKIO_RADIO, Reachability::Reachable),
        Ok(TopologyMutation::Applied)
    );
    assert!(tasks.settle() > 0);
    let desktop = desktop.try_recv().unwrap();
    converge(&mut tasks, &lab, &embedded, &desktop.handle);
    let links = establish_pair(&mut tasks, &embedded, &desktop.handle);
    run(&mut tasks, &lab, &embedded, &desktop, links);
    assert!(embedded.take_closed().is_empty());
    assert!(desktop.take_closed().is_empty());
    assert!(embedded.take_received().is_empty());
    assert!(embedded.responses.is_empty());
    assert!(desktop.responses.is_empty());
    assert!(embedded.wire.is_idle());
    assert!(desktop.wire.is_idle());
    drop(tasks);
    assert_radio_cleanup(&lab);
}

fn reconnect(
    tasks: &mut EmbassyTasks<'_>,
    lab: &VirtualBleLab,
    embedded: &ResourceNode,
    desktop: &tokio_node::TokioNode,
    links: [LinkId; 2],
) -> [LinkId; 2] {
    assert_eq!(
        lab.set_reachability(EMBASSY_RADIO, TOKIO_RADIO, Reachability::Reachable),
        Ok(TopologyMutation::Applied)
    );
    converge(tasks, lab, embedded, &desktop.handle);
    let mut expired: Vec<_> = links
        .into_iter()
        .map(|link| (link, LinkClosedReason::Timeout))
        .collect();
    expired.sort_by_key(|(link, _)| *link.as_bytes());
    assert_eq!(embedded.take_closed(), expired);
    assert_eq!(desktop.take_closed(), expired);
    let fresh = establish_pair(tasks, embedded, &desktop.handle);
    for (old, new) in links.into_iter().zip(fresh) {
        assert_ne!(old, new);
    }
    fresh
}

fn reassemble(
    tasks: &mut EmbassyTasks<'_>,
    _lab: &VirtualBleLab,
    embedded: &ResourceNode,
    desktop: &tokio_node::TokioNode,
    links: [LinkId; 2],
) {
    for case in [AdmissionCase::RefusedAtOffer, AdmissionCase::ThreeSegments] {
        journaled_request(
            tasks,
            &desktop.handle,
            desktop.responses.clone(),
            links[1],
            &case,
        );
        let responder = match case {
            AdmissionCase::RefusedAtOffer => Settlement::Respond(Err(RespondFailure::Resource(
                SendResourceFailure::RejectedByPeer,
            ))),
            AdmissionCase::ThreeSegments => Settlement::Respond(Ok(())),
        };
        assert_eq!(response_settlements(embedded), [responder]);
        let expected = journaled_request(
            tasks,
            &embedded.handle,
            embedded.responses.clone(),
            links[0],
            &case,
        );
        assert_eq!(embedded.take_settled(), [expected]);
    }
    for _ in 0..2 {
        let handle = desktop.handle.clone();
        let (result, elapsed) = complete(tasks, async move {
            measured(handle.request(
                links[1],
                RequestPathHash::of(files::FILE_PATH),
                &(TRANSFER_BYTES as u16).to_be_bytes(),
            ))
            .await
        });
        assert_eq!(
            result,
            Ok((files::FILE_BYTES[..TRANSFER_BYTES].to_vec(), elapsed))
        );
        assert_eq!(
            response_settlements(embedded),
            [Settlement::Respond(Ok(()))]
        );
        let handle = embedded.handle;
        let (result, elapsed) = complete(tasks, async move {
            measured(handle.request(
                links[0],
                RequestPathHash::of(files::FILE_PATH),
                &(TRANSFER_BYTES as u16).to_be_bytes(),
            ))
            .await
        });
        assert_eq!(
            result.map(|(bytes, rtt)| (bytes.as_slice().to_vec(), rtt)),
            Ok((files::FILE_BYTES[..TRANSFER_BYTES].to_vec(), elapsed))
        );
        assert!(response_settlements(embedded).is_empty());
    }
}

#[test]
fn esp32_and_apple_nodes_reassemble_segmented_responses() {
    scenario(
        Endpoint::Esp32(Esp32Host::Esp32),
        Endpoint::CoreBluetooth(AppleHost::MacOs),
        TRACE_CAPACITY,
        reassemble,
    );
}

#[test]
fn nrf52_and_bluez_nodes_reassemble_segmented_responses() {
    scenario(
        Endpoint::Nrf52(Nrf52Host::Nrf52),
        Endpoint::BlueZ(BlueZHost::Linux),
        TRACE_CAPACITY,
        reassemble,
    );
}
