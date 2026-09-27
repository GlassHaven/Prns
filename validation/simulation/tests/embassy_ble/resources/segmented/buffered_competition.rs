use super::*;
use crate::request_probe::RequestProbe;
use crate::wire_gate::WireGate;
use personal_rns::engine::{Respond, RespondData, RespondPayload};
use personal_rns::wire::{WireContext, WirePacketHeader};
use std::cell::Cell;

#[derive(Clone, Copy)]
enum Phase {
    BeforeFirstSegment,
    BetweenSegments,
    DuringContinuation,
}

const COMPETING_BYTES: &[u8] = b"not the requested file";
const BARRIER_BYTES: &[u8] = b"independent request on the same link";
const FIRST_SEGMENT_WIRE_PARTS: usize = 2;

async fn compete<T>(
    responder: &impl PrnsNodeApi,
    probe: &RequestProbe,
    gate: &WireGate,
    link: LinkId,
    phase: &Phase,
    request: impl Future<Output = T>,
    barrier: impl Future<Output = ()>,
) -> T {
    let advertisement = buffered_interruption::advertisement_header(link);
    let part = WirePacketHeader {
        context: WireContext::Resource,
        ..advertisement
    };
    let (header, passes) = match phase {
        Phase::BeforeFirstSegment => (part, 0),
        Phase::BetweenSegments => (advertisement, 1),
        Phase::DuringContinuation => (part, FIRST_SEGMENT_WIRE_PARTS),
    };
    probe.arm(link, RequestPathHash::of(files::FILE_PATH));
    gate.lose_after(header, passes, NonZeroUsize::new(16).unwrap());
    let completed = Cell::new(false);
    let (result, ()) = tokio::join!(
        async {
            let result = request.await;
            completed.set(true);
            result
        },
        async {
            let (observed_link, request_id) = probe.take().await;
            assert_eq!(observed_link, link);
            assert_eq!(gate.first_loss().await, header);
            assert!(responder
                .issue(PrnsCommand::Respond(Respond {
                    link_id: link,
                    request_id,
                    payload: RespondPayload::Packed(
                        RespondData::from_slice(COMPETING_BYTES).unwrap()
                    ),
                }))
                .is_some());
            barrier.await;
            assert!(
                !completed.get(),
                "competing packet must not complete the buffered request"
            );
            assert!(gate.stop_loss() > 0);
        },
    );
    assert!(probe.is_idle());
    assert!(gate.is_idle());
    result
}

fn exercise(phase: Phase, requester: Requester) {
    for (embedded_endpoint, desktop_endpoint) in [
        (
            Endpoint::Esp32(Esp32Host::Esp32),
            Endpoint::CoreBluetooth(AppleHost::MacOs),
        ),
        (
            Endpoint::Nrf52(Nrf52Host::Nrf52),
            Endpoint::BlueZ(BlueZHost::Linux),
        ),
    ] {
        scenario(
            embedded_endpoint,
            desktop_endpoint,
            TRACE_CAPACITY,
            |tasks, lab, embedded, desktop, links| {
                match requester {
                    Requester::Tokio => {
                        let desktop_handle = desktop.handle.clone();
                        let responder = embedded.handle;
                        let probe = embedded.requests.clone();
                        let gate = embedded.wire.clone();
                        let (result, elapsed) = complete(tasks, async move {
                            let data = (TRANSFER_BYTES as u16).to_be_bytes();
                            measured(compete(
                                &responder,
                                &probe,
                                &gate,
                                links[1],
                                &phase,
                                desktop_handle.request_with_options(
                                    links[1],
                                    RequestPathHash::of(files::FILE_PATH),
                                    &data,
                                    RequestOptions {
                                        response_timeout: RequestResponseTimeout::LinkDefault,
                                        maximum_response_bytes: ByteLimit::Maximum(
                                            TRANSFER_BYTES as u64,
                                        ),
                                    },
                                ),
                                async {
                                    let (result, elapsed) = measured(desktop_handle.request(
                                        links[1],
                                        RequestPathHash::of(crate::echo::QUERY_PATH),
                                        BARRIER_BYTES,
                                    ))
                                    .await;
                                    assert_eq!(result, Ok((BARRIER_BYTES.to_vec(), elapsed)));
                                },
                            ))
                            .await
                        });
                        assert_eq!(
                            result,
                            Ok((files::FILE_BYTES[..TRANSFER_BYTES].to_vec(), elapsed))
                        );
                        assert_eq!(
                            response_settlements(embedded),
                            [const { Settlement::Respond(Ok(())) }; 3]
                        );
                        assert!(desktop.responses.is_empty());
                    }
                    Requester::Embassy => {
                        let handle = embedded.handle;
                        let responder = desktop.handle.clone();
                        let probe = desktop.requests.clone();
                        let gate = desktop.wire.clone();
                        let (result, elapsed) = complete(tasks, async move {
                            let data = (TRANSFER_BYTES as u16).to_be_bytes();
                            measured(compete(
                                &responder,
                                &probe,
                                &gate,
                                links[0],
                                &phase,
                                handle.request(
                                    links[0],
                                    RequestPathHash::of(files::FILE_PATH),
                                    &data,
                                ),
                                async {
                                    let (result, elapsed) = measured(handle.request(
                                        links[0],
                                        RequestPathHash::of(crate::echo::QUERY_PATH),
                                        BARRIER_BYTES,
                                    ))
                                    .await;
                                    assert_eq!(
                                        result.map(|(bytes, rtt)| (bytes.as_slice().to_vec(), rtt)),
                                        Ok((BARRIER_BYTES.to_vec(), elapsed)),
                                    );
                                },
                            ))
                            .await
                        });
                        assert_eq!(
                            result.map(|(bytes, rtt)| (bytes.as_slice().to_vec(), rtt)),
                            Ok((files::FILE_BYTES[..TRANSFER_BYTES].to_vec(), elapsed)),
                        );
                        assert!(embedded.take_settled().is_empty());
                        assert!(embedded.responses.is_empty());
                    }
                }
                reuse_both_embassy_request_slots(tasks, embedded, links[0]);
                reassemble(tasks, lab, embedded, desktop, links);
            },
        );
    }
}

#[test]
fn tokio_buffered_competition_before_first_segment() {
    exercise(Phase::BeforeFirstSegment, Requester::Tokio);
}

#[test]
fn embassy_buffered_competition_before_first_segment() {
    exercise(Phase::BeforeFirstSegment, Requester::Embassy);
}

#[test]
fn tokio_buffered_competition_between_segments() {
    exercise(Phase::BetweenSegments, Requester::Tokio);
}

#[test]
fn embassy_buffered_competition_between_segments() {
    exercise(Phase::BetweenSegments, Requester::Embassy);
}

#[test]
fn tokio_buffered_competition_during_continuation() {
    exercise(Phase::DuringContinuation, Requester::Tokio);
}

#[test]
fn embassy_buffered_competition_during_continuation() {
    exercise(Phase::DuringContinuation, Requester::Embassy);
}
