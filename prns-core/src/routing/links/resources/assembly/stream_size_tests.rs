use super::*;
use crate::routing::links::resources::{ResourceHash, ResourceSegment};
use crate::routing::links::LinkId;

fn check_size<C: IncomingAssemblyTable + Default>(original: u64, offered: u64) {
    let mut assemblies = IncomingAssemblies::<C>::default();
    let link = LinkId::new([1; 16]);
    let hash = ResourceHash::new([2; 32]);
    let correlation = AssemblyCorrelation::Unsolicited;
    assemblies.begin(link, hash, 2, original, correlation);
    let first = ResourceSegment {
        index: 1,
        total_segments: 2,
        total_data_bytes: original,
    };
    assert_eq!(
        assemblies.advance(&link, &hash, first, 0, correlation),
        Some(AssemblyProgress::Assembling)
    );
    let next = ResourceSegment {
        index: 2,
        total_segments: 2,
        total_data_bytes: offered,
    };
    let expected = if original == offered {
        SegmentFit::Expected
    } else {
        SegmentFit::Unexpected
    };
    assert_eq!(assemblies.fit(&link, &hash, next, correlation), expected);
    let advanced = assemblies.advance(&link, &hash, next, 0, correlation);
    assert_eq!(
        advanced,
        if original == offered {
            Some(AssemblyProgress::Complete {
                total_size_bytes: 0,
            })
        } else {
            None
        }
    );
    if original != offered {
        assert_eq!(
            assemblies.fit(
                &link,
                &hash,
                ResourceSegment {
                    total_data_bytes: original,
                    ..next
                },
                correlation
            ),
            SegmentFit::Expected
        );
    }
}

proptest::proptest! {
    #[test]
    fn stream_size_identity_is_required_by_fit_and_advance(original in proptest::prelude::any::<u64>(), offered in proptest::prelude::any::<u64>()) {
        for size in [0, original, offered, u64::MAX] {
            check_size::<FixedIncomingAssemblyTable<2>>(original, size);
            #[cfg(feature = "alloc")]
            check_size::<HeapIncomingAssemblyTable>(original, size);
        }
    }
}

fn check_column_lifecycle<C: IncomingAssemblyTable + Default>() {
    let mut assemblies = IncomingAssemblies::<C>::default();
    let first = LinkId::new([1; 16]);
    let second = LinkId::new([2; 16]);
    let hash = ResourceHash::new([3; 32]);
    let correlation = AssemblyCorrelation::Unsolicited;
    assemblies.begin(first, hash, 2, 128, correlation);
    assemblies.begin(second, hash, 2, 256, correlation);
    assemblies.clear(&first);
    for (link, size, expected) in [
        (first, 128, SegmentFit::Unexpected),
        (second, 256, SegmentFit::Expected),
        (second, 128, SegmentFit::Unexpected),
    ] {
        assert_eq!(
            assemblies.fit(
                &link,
                &hash,
                ResourceSegment {
                    index: 1,
                    total_segments: 2,
                    total_data_bytes: size
                },
                correlation
            ),
            expected
        );
    }
    assemblies.begin(first, hash, 2, 0, correlation);
    assemblies.begin(second, hash, 2, u64::MAX, correlation);
    for (link, size) in [(first, 0), (second, u64::MAX)] {
        assert_eq!(
            assemblies.fit(
                &link,
                &hash,
                ResourceSegment {
                    index: 1,
                    total_segments: 2,
                    total_data_bytes: size
                },
                correlation
            ),
            SegmentFit::Expected
        );
    }
}

#[test]
fn stream_size_columns_survive_swap_removal_reuse_and_replacement() {
    check_column_lifecycle::<FixedIncomingAssemblyTable<2>>();
    #[cfg(feature = "alloc")]
    check_column_lifecycle::<HeapIncomingAssemblyTable>();
}
