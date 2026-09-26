use super::*;

const SEGMENT_BYTES: usize = 128;
const STREAM_BYTES: u64 = (2 * SEGMENT_BYTES) as u64;

fn check_stream_size(opening: Opening, advertised: u64) {
    let mut receiver = engine_with_active_link();
    let mut sender = engine_with_active_link();
    let request = track_pending_request(&mut receiver, CommandId(42), 1_800, 20_000);
    let mut all = Delivery::default();
    for index in 1..=2 {
        let data = [0xA0 + index as u8; SEGMENT_BYTES];
        let at = 2_000 + (index - 1) * 1_000;
        let advertisement = advertise_response_segment_from(
            &mut sender,
            CommandId(20 + index),
            request,
            &data,
            opening.candidate(),
            ResourceSegment {
                index,
                total_segments: 2,
                total_data_bytes: STREAM_BYTES,
            },
            at,
        );
        let advertisement = rewrite_advertisement(&advertisement, |ad| ad.data_bytes = advertised);
        let pull = feed(&mut receiver, &advertisement, at + 100);
        assert_eq!(pull.frames.len(), 1);
        let hash = receiver
            .incoming_resources
            .first_hash_for_link(&link_id())
            .unwrap();
        let served = feed(&mut sender, &pull.frames[0].1, at + 200);
        assert_eq!(served.frames.len(), 1);
        let mut delivered = Delivery::default();
        let proofs = finish_segment(
            &mut receiver,
            &served.frames,
            &data,
            &opening,
            at + 300,
            &mut delivered,
        );
        let cumulative = index * SEGMENT_BYTES as u64;
        let valid = cumulative <= advertised && (index < 2 || cumulative == advertised);
        if !valid {
            assert!(
                proofs.is_empty(),
                "a false stream size must not receive a proof"
            );
            assert_eq!(
                delivered,
                expected_failure(hash, ResourceFailureCause::TransferCorrupt)
            );
            assert_retired(&receiver, request);
            for (_, part) in &served.frames {
                assert_eq!(
                    feed(&mut receiver, part, at + 500),
                    InboundCapture::default()
                );
            }
            return;
        }
        assert_eq!(proofs.len(), 1);
        for (_, proof) in proofs {
            feed(&mut sender, &proof, at + 400);
        }
        all.chunks.extend(delivered.chunks);
        all.settlements.extend(delivered.settlements);
        all.failures.extend(delivered.failures);
    }
    let finished_at = match opening {
        Opening::Uncompressed => 3_300,
        Opening::Inflated => 3_310,
    };
    assert_eq!(
        all,
        Delivery {
            chunks: std::vec![
                (CommandId(42), request, 1, std::vec![0xA1; SEGMENT_BYTES]),
                (CommandId(42), request, 2, std::vec![0xA2; SEGMENT_BYTES]),
            ],
            settlements: std::vec![(
                CommandId(42),
                Settlement::SendRequest(Ok(PacketReceiptDelivered {
                    rtt: RttMillis::new(finished_at - 1_800),
                    evidence: DeliveryEvidence::Response,
                }))
            )],
            failures: std::vec![],
        }
    );
    assert_retired(&receiver, request);
}

#[test]
fn opened_segments_enforce_cumulative_advertised_stream_size() {
    for advertised in [
        0,
        SEGMENT_BYTES as u64 - 1,
        SEGMENT_BYTES as u64,
        STREAM_BYTES - 1,
        STREAM_BYTES,
        STREAM_BYTES + 1,
    ] {
        check_stream_size(Opening::Uncompressed, advertised);
    }
}

#[test]
fn inflated_segments_enforce_cumulative_advertised_stream_size() {
    for advertised in [
        0,
        SEGMENT_BYTES as u64 - 1,
        SEGMENT_BYTES as u64,
        STREAM_BYTES - 1,
        STREAM_BYTES,
        STREAM_BYTES + 1,
    ] {
        check_stream_size(Opening::Inflated, advertised);
    }
}
