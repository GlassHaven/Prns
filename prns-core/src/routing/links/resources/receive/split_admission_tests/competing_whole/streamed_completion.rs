use super::*;
use crate::engine::test_support::routable_descriptor;
use crate::engine::{
    IngestIo, OpenedResourceSpan, OwedWork, ResourceOpenCompleted, ResourceOpenSpanResidence,
};
use crate::interfaces::{AttachedInterfaces, InboundPacket};
use crate::routing::links::resources::streamed_open::StreamedOpen;

struct PendingSpan {
    hash: ResourceHash,
    start: usize,
    state: StreamedOpen,
    bytes: std::vec::Vec<u8>,
    residence: ResourceOpenSpanResidence,
}

enum SplitPhase {
    BetweenSegments,
    ReceivingContinuation,
}

#[test]
fn delayed_streamed_open_cannot_publish_over_a_split_response() {
    for phase in [
        SplitPhase::BetweenSegments,
        SplitPhase::ReceivingContinuation,
    ] {
        let mut receiver = engine_with_active_link();
        let request = track_pending_request(&mut receiver, REQUEST, 1_800, 20_000);
        let mut competitor = engine_with_active_link();
        let body = four_part_payload();
        let advertisement = advertise_response_segment_from(
            &mut competitor,
            CommandId(99),
            request,
            &body,
            None,
            ResourceSegment::whole(body.len() as u64),
            1_810,
        );
        let pull = feed(&mut receiver, &advertisement, 1_820);
        assert_eq!(pull.frames.len(), 1);
        let served = feed(&mut competitor, &pull.frames[0].1, 1_830);
        assert_eq!(served.frames.len(), 4);
        let mut job = None;
        for (index, (_, frame)) in served.frames.iter().enumerate() {
            let at = InstantMillis(1_840 + index as u64);
            let mut bytes = frame.clone();
            receiver.ingest_packet_into(
                InboundPacket {
                    arrived_at: at,
                    source_interface: lane(),
                    bytes: &mut bytes,
                },
                IngestIo {
                    interfaces: AttachedInterfaces::new(&[routable_descriptor(lane())]),
                    now: at,
                    fill_random: &mut |bytes| bytes.fill(0xC7),
                    should_prove: &mut |_| false,
                    should_accept_resource: &mut |_| false,
                    sink: &mut |reaction| match reaction {
                        EngineReaction::Directive(Directive::Fulfill(OwedWork::ResourceOpen(
                            owed,
                        ))) => {
                            assert_eq!(index, 0);
                            assert!(job.is_none());
                            assert!(!owed.other_transfers_in_flight);
                            job = Some(PendingSpan {
                                hash: owed.hash,
                                start: owed.span_start,
                                state: owed.state,
                                bytes: owed.bytes.to_vec(),
                                residence: owed.residence,
                            });
                        }
                        _ => panic!("parts arriving behind a parked open must not publish"),
                    },
                },
            );
        }
        let mut pending = job.expect("first ciphertext span is held by worker");
        assert_eq!(pending.residence, ResourceOpenSpanResidence::Resident);
        let mut response = SplitResponse::from_pending(receiver, request);
        let mut continuation = None;
        if let SplitPhase::ReceivingContinuation = phase {
            let pull = feed(&mut response.receiver, &response.continuation, 2_350);
            assert_eq!(pull.frames.len(), 1);
            continuation = Some(pull.frames[0].1.clone());
        }
        let deadline = response
            .receiver
            .receipts
            .pending_request_deadline(&link_id(), request);
        let mut capture = InboundCapture::default();
        let mut completions = 0;
        loop {
            completions += 1;
            assert!(
                completions <= 2,
                "the held span and at most one remaining span"
            );
            pending.state.chew_span(&mut pending.bytes);
            let mut next = None;
            response.receiver.resume_resource_open(
                ResourceOpenCompleted {
                    link_id: link_id(),
                    hash: pending.hash,
                    span_start: pending.start,
                    state: pending.state,
                    opened: OpenedResourceSpan::Returned(&pending.bytes),
                    residence: pending.residence,
                },
                InstantMillis(2_400 + completions),
                &mut |bytes| bytes.fill(0xC9),
                &mut |reaction| match reaction {
                    EngineReaction::Directive(Directive::Fulfill(OwedWork::ResourceOpen(owed))) => {
                        assert!(next.is_none());
                        assert!(owed.other_transfers_in_flight);
                        next = Some(PendingSpan {
                            hash: owed.hash,
                            start: owed.span_start,
                            state: owed.state,
                            bytes: owed.bytes.to_vec(),
                            residence: owed.residence,
                        });
                    }
                    EngineReaction::Directive(Directive::EmitFrame { target, fill, .. }) => {
                        capture.frames.push((target, filled_frame(fill).unwrap()))
                    }
                    _ => panic!("streamed competitor must never publish or settle the request"),
                },
            );
            match next {
                Some(job) => pending = job,
                None => break,
            }
        }
        assert_eq!(
            completions,
            match phase {
                SplitPhase::BetweenSegments => 1,
                SplitPhase::ReceivingContinuation => 2,
            }
        );
        assert_cancelled(&mut competitor, capture, 2_450);
        assert_eq!(
            response
                .receiver
                .receipts
                .pending_request_deadline(&link_id(), request),
            deadline
        );
        let pull = match continuation {
            Some(pull) => pull,
            None => {
                let pull = feed(&mut response.receiver, &response.continuation, 2_500);
                assert_eq!(pull.frames.len(), 1);
                pull.frames[0].1.clone()
            }
        };
        response.complete(&pull);
    }
}
