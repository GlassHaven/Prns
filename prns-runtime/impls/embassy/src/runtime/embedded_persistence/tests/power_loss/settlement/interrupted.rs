use super::*;

#[test]
fn interrupted_pairing_rollback_exposes_the_durable_authority_commit_boundary() {
    let healthy = verify(Finalize::HealthyRollback);
    let commit = healthy
        .trace
        .iter()
        .rposition(|event| matches!(event, Operation::Write { .. }))
        .unwrap();
    assert!(matches!(
        healthy.trace[commit],
        Operation::Write { len: 4, .. }
    ));
    let prior =
        crate::runtime::node_facade::test_remote_control_grant(RemoteControlRequestKind::Describe);
    let candidate = crate::runtime::node_facade::test_remote_control_grant(
        RemoteControlRequestKind::AnnounceSelf,
    );
    let mut cuts = 0;
    let mut restored_candidate = 0;
    for (operation, event) in healthy.trace.iter().enumerate() {
        let prefixes: Vec<_> = match event {
            Operation::Read { len, .. } => std::vec![0, *len],
            Operation::Write { len, .. } => (0..=*len).collect(),
            Operation::Erase { .. } => panic!("rollback fixture has room without compaction"),
        };
        for completed_bytes in prefixes {
            let cut = Cut {
                operation,
                completed_bytes,
            };
            let interrupted = verify(Finalize::InterruptedRollback(cut));
            assert_eq!(interrupted.trace, healthy.trace[..=operation], "{cut:?}");
            let expected = if operation > commit || (operation == commit && completed_bytes == 4) {
                prior
            } else {
                restored_candidate += 1;
                candidate
            };
            for _ in 0..2 {
                embassy_futures::block_on(super::super::grants::restore(
                    interrupted.image,
                    &[expected],
                ));
            }
            cuts += 1;
        }
    }
    assert!(restored_candidate > 0 && restored_candidate < cuts);
    std::eprintln!("verified {cuts} rollback cuts; {restored_candidate} restore candidate authority before rollback commit");
}
