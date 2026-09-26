use super::*;
use crate::engine::{NoOwedWork, ResourceDecompressionCompleted};

enum Completion {
    Uncompressed,
    DeferredInflate,
}

#[test]
fn preadmitted_whole_completion_cannot_publish_over_a_split_response() {
    for completion in [Completion::Uncompressed, Completion::DeferredInflate] {
        let mut receiver = engine_with_active_link();
        let request = track_pending_request(&mut receiver, REQUEST, 1_800, 20_000);
        let mut competitor = engine_with_active_link();
        let body = [0xE7; 128];
        let candidate = match completion {
            Completion::Uncompressed => None,
            Completion::DeferredInflate => Some(b"worker-owned compressed bytes".as_slice()),
        };
        let advertisement = advertise_response_segment_from(
            &mut competitor,
            CommandId(99),
            request,
            &body,
            candidate,
            ResourceSegment::whole(128),
            1_810,
        );
        let pull = feed(&mut receiver, &advertisement, 1_820);
        assert_eq!(pull.frames.len(), 1);
        let hash = receiver
            .incoming_resources
            .first_hash_for_link(&link_id())
            .unwrap();
        let served = feed(&mut competitor, &pull.frames[0].1, 1_830);
        assert_eq!(served.frames.len(), 1);
        if let Completion::DeferredInflate = completion {
            assert_eq!(
                feed(&mut receiver, &served.frames[0].1, 1_840),
                InboundCapture::default()
            );
        }
        let mut response = SplitResponse::from_pending(receiver, request);
        let deadline = response
            .receiver
            .receipts
            .pending_request_deadline(&link_id(), request);
        let rejected = match completion {
            Completion::Uncompressed => feed(&mut response.receiver, &served.frames[0].1, 2_400),
            Completion::DeferredInflate => {
                let mut capture = InboundCapture::default();
                response.receiver.resume_resource_decompression(
                    ResourceDecompressionCompleted {
                        link_id: link_id(),
                        hash,
                        plaintext: &body,
                    },
                    InstantMillis(2_400),
                    &mut |bytes| bytes.fill(0xC9),
                    &mut |reaction: EngineReaction<'_, NoOwedWork>| match reaction {
                        EngineReaction::Directive(Directive::EmitFrame {
                            target, fill, ..
                        }) => capture.frames.push((target, filled_frame(fill).unwrap())),
                        _ => panic!("superseded completion must only emit cancellation"),
                    },
                );
                capture
            }
        };
        assert_cancelled(&mut competitor, rejected, 2_450);
        assert_eq!(
            response
                .receiver
                .receipts
                .pending_request_deadline(&link_id(), request),
            deadline
        );
        assert!(response.receiver.incoming_resources.is_empty());
        let pull = feed(&mut response.receiver, &response.continuation, 2_500);
        assert_eq!(pull.frames.len(), 1);
        response.complete(&pull.frames[0].1);
    }
}
