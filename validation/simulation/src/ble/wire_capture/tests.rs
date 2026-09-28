use super::*;

#[test]
fn eviction_retains_whole_values_and_snapshots_are_independent() {
    let capture = BleWireCapture::new(
        NonZeroUsize::new(2).unwrap_or_else(|| unreachable!("nonzero capacity")),
    );
    let first = BleAddress::new([1; 6]);
    let second = BleAddress::new([2; 6]);
    capture.record(first, second, BleWireChannel::Control, &[3, 0]);
    let before = capture.snapshot();
    let shared = capture.clone();
    shared.record(second, first, BleWireChannel::Data, &[1, 2, 3]);
    shared.record(first, second, BleWireChannel::Control, &[]);
    assert_eq!(
        before,
        BleWireSnapshot {
            discarded_values: 0,
            values: vec![BleWireValue {
                from: first,
                to: second,
                channel: BleWireChannel::Control,
                bytes: vec![3, 0]
            }],
        }
    );
    assert_eq!(
        capture.snapshot(),
        BleWireSnapshot {
            discarded_values: 1,
            values: vec![
                BleWireValue {
                    from: second,
                    to: first,
                    channel: BleWireChannel::Data,
                    bytes: vec![1, 2, 3]
                },
                BleWireValue {
                    from: first,
                    to: second,
                    channel: BleWireChannel::Control,
                    bytes: vec![]
                },
            ],
        }
    );
}
