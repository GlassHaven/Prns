use super::*;
use crate::engine::{
    PrnsCommand, SettleRemoteControlTargetPairingAuthorizationFailure as Failure, Settlement,
};
use crate::manifold::driver::HostCommand;
use crate::persistence::{
    read_remote_control_controller_grants_snapshot, FileStore, PersistedStore,
};
use crate::runtime::node_facade::{NodePersistence, TestDirectory};

enum Acknowledgement {
    Lost,
    NodeStopped,
    Rejected(Failure),
    Inconsistent,
}

#[tokio::test]
async fn committed_target_grants_survive_unavailable_and_rejected_settlement() {
    let attempt_id = RemoteControlPairingAttemptId::from_test_transcript_digest_bytes([0x81; 32]);
    for acknowledgement in [
        Acknowledgement::Lost,
        Acknowledgement::NodeStopped,
        Acknowledgement::Inconsistent,
        Acknowledgement::Rejected(Failure::NoAuthorizationOwed {
            settled: attempt_id,
        }),
        Acknowledgement::Rejected(Failure::AttemptMismatch {
            settled: attempt_id,
            active: RemoteControlPairingAttemptId::from_test_transcript_digest_bytes([0x82; 32]),
        }),
        Acknowledgement::Rejected(Failure::TargetSignerUnavailable {
            attempt_id,
            target_identity: crate::identity::IdentityHash::new([0x83; 16]),
        }),
        Acknowledgement::Rejected(Failure::CompletionSigningFailed {
            attempt_id,
            error: crate::remote_control::RemoteControlPairingCompletionSigningError::TargetIdentityMismatch {
                expected: crate::identity::IdentityHash::new([0x83; 16]),
                found: crate::identity::IdentityHash::new([0x84; 16]),
            },
        }),
    ] {
        for prior in [None, Some(RemoteControlRequestKind::Describe)] {
            let directory = TestDirectory::new();
            let (commands, mut receiver) = mpsc::unbounded_channel();
            let node = PrnsNodeHandle::over(commands);
            let worker = NodePersistence::custom_dir(directory.path())
                .unwrap()
                .worker(node.clone());
            let persistence = worker.remote_control_authorization_persistence();
            let mut remote = remote_control();
            let grant = crate::runtime::node_facade::test_remote_control_grant;
            if let Some(request) = prior {
                remote.set_controller_grant(grant(request)).unwrap();
            }
            let candidate = grant(RemoteControlRequestKind::AnnounceSelf);
            persistence
                .store(
                    SnapshotRegion::RemoteControlControllerGrants,
                    controller_grants_snapshot(&remote).unwrap(),
                )
                .await
                .unwrap();
            let apply = persist_controller_grant(
                &mut remote,
                Some(&persistence),
                &node,
                attempt_id,
                candidate,
            );
            let acknowledge = async {
                if matches!(acknowledgement, Acknowledgement::NodeStopped) {
                    drop(receiver);
                    return;
                }
                let Some(HostCommand::AwaitedEngine { issued, completion }) = receiver.recv().await
                else {
                    panic!("awaited target pairing settlement");
                };
                assert_eq!(
                    issued.command,
                    PrnsCommand::SettleRemoteControlTargetPairingAuthorization(
                        SettleRemoteControlTargetPairingAuthorization {
                            attempt_id,
                            persistence:
                                RemoteControlTargetPairingAuthorizationPersistence::Persisted,
                        }
                    )
                );
                let store = FileStore::new(directory.path());
                let mut bytes = vec![0; remote_control_controller_grants_snapshot_capacity(1)];
                let loaded = store
                    .load(SnapshotRegion::RemoteControlControllerGrants, &mut bytes)
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    read_remote_control_controller_grants_snapshot(loaded)
                        .unwrap()
                        .collect::<Vec<_>>(),
                    vec![candidate]
                );
                match acknowledgement {
                    Acknowledgement::Inconsistent => completion.send(Settlement::SettleRemoteControlTargetPairingAuthorization(Ok(
                        RemoteControlTargetPairingFinalization::AuthorizationFailureRecorded {
                            attempt_id,
                            retired_link: crate::routing::links::LinkId::new([0x85; 16]),
                            responder: crate::remote_control::RemoteControlTargetPairingResponder::new(
                                crate::routing::links::LinkId::new([0x85; 16]),
                                crate::routing::links::request::RequestId([0x86; 16]),
                            ),
                        }
                    ))).unwrap(),
                    Acknowledgement::Rejected(failure) => completion
                        .send(Settlement::SettleRemoteControlTargetPairingAuthorization(
                            Err(failure),
                        ))
                        .unwrap(),
                    Acknowledgement::Lost | Acknowledgement::NodeStopped => drop(completion),
                }
            };
            let (result, ()) = tokio::join!(apply, acknowledge);
            assert_eq!(result, Err(match acknowledgement {
                Acknowledgement::Inconsistent => RemoteControlAuthorizationPersistenceFailure::CommittedTargetGrantFinalizationMismatch,
                Acknowledgement::Rejected(failure) => RemoteControlAuthorizationPersistenceFailure::CommittedTargetGrantSettlement { failure },
                Acknowledgement::Lost | Acknowledgement::NodeStopped => RemoteControlAuthorizationPersistenceFailure::CommittedTargetGrantSettlementUnavailable,
            }));
            assert_eq!(
                remote
                    .controller_grants()
                    .unwrap()
                    .grants_in_identity_hash_order(),
                &[candidate]
            );
            for _ in 0..2 {
                let store = FileStore::new(directory.path());
                let mut bytes = vec![0; remote_control_controller_grants_snapshot_capacity(1)];
                let loaded = store
                    .load(SnapshotRegion::RemoteControlControllerGrants, &mut bytes)
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    read_remote_control_controller_grants_snapshot(loaded)
                        .unwrap()
                        .collect::<Vec<_>>(),
                    vec![candidate]
                );
            }
        }
    }
}
