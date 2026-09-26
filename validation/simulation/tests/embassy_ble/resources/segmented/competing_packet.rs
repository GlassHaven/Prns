use super::*;
use crate::wire_gate::WireGate;
use personal_rns::engine::{Respond, RespondData, RespondPayload};

fn compete(
    tasks: &mut EmbassyTasks<'_>,
    requester: &impl PrnsNodeApi,
    responder: &impl PrnsNodeApi,
    trace: ResponseTrace,
    gate: &WireGate,
    link: LinkId,
) -> (CommandId, Settlement) {
    gate.lose_after(
        buffered_interruption::advertisement_header(link),
        1,
        NonZeroUsize::new(16).unwrap(),
    );
    let started = tasks.snapshot().tick;
    let command = requester
        .issue(PrnsCommand::SendRequest(SendRequest {
            link_id: link,
            path_hash: RequestPathHash::of(files::FILE_PATH),
            data: SendRequestData::from_slice(&(TRANSFER_BYTES as u16).to_be_bytes()).unwrap(),
            response_timeout: RequestResponseTimeout::LinkDefault,
            maximum_response_bytes: ByteLimit::Maximum(TRANSFER_BYTES as u64),
        }))
        .unwrap();
    let observer = trace.clone();
    let first = complete(tasks, async move { observer.next().await });
    let ResponseEvent::Segment {
        request,
        index: 1,
        total: 3,
        ..
    } = &first
    else {
        unreachable!("the split response must own the request before competing")
    };
    let request = *request;
    assert!(trace.is_empty());
    assert!(responder
        .issue(PrnsCommand::Respond(Respond {
            link_id: link,
            request_id: request,
            payload: RespondPayload::Packed(RespondData::from_slice(b"competing packet").unwrap()),
        }))
        .is_some());
    tasks.settle();
    assert!(
        trace.is_empty(),
        "a competing packet must not publish or settle a split request"
    );
    assert!(gate.stop_loss() > 0);
    let mut events = vec![first];
    events.extend(complete(tasks, async move { trace.completed().await }));
    let result = Ok(PacketReceiptDelivered {
        rtt: RttMillis::new(tasks.snapshot().tick.get() - started.get()),
        evidence: DeliveryEvidence::Response,
    });
    assert_eq!(events.len(), 4);
    let mut expected = Vec::new();
    let mut offset = 0;
    for (position, event) in events[..3].iter().enumerate() {
        let ResponseEvent::Segment { bytes, .. } = event else {
            unreachable!("the original response must retain its delivery lane")
        };
        let end = offset + bytes.len();
        expected.push(ResponseEvent::Segment {
            link,
            request,
            index: position as u64 + 1,
            total: 3,
            bytes: files::FILE_BYTES[offset..end].to_vec(),
        });
        offset = end;
    }
    assert_eq!(offset, TRANSFER_BYTES);
    expected.push(ResponseEvent::Settled { command, result });
    assert_eq!(events, expected);
    (command, Settlement::SendRequest(result))
}

#[test]
fn packet_responses_cannot_replace_an_active_split_response() {
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
                compete(
                    tasks,
                    &desktop.handle,
                    &embedded.handle,
                    desktop.responses.clone(),
                    &embedded.wire,
                    links[1],
                );
                assert_eq!(
                    response_settlements(embedded),
                    [Settlement::Respond(Ok(())), Settlement::Respond(Ok(()))]
                );
                let expected = compete(
                    tasks,
                    &embedded.handle,
                    &desktop.handle,
                    embedded.responses.clone(),
                    &desktop.wire,
                    links[0],
                );
                assert_eq!(embedded.take_settled(), [expected]);
                reassemble(tasks, lab, embedded, desktop, links);
            },
        );
    }
}
