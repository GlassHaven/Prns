use super::*;
use crate::identity::{IdentityEncryptionPublicKey, IdentityPublicKeys, IdentitySigningPublicKey};
use crate::persistence::{
    read_remote_control_controller_grants_snapshot, FileStore, PersistedStore, SnapshotRegion,
};
use crate::remote_control::{
    RemoteControlControllerAuthority, RemoteControlControllerGrant,
    RemoteControlControllerIdentity, RemoteControlRequestSet,
};
use crate::runtime::node_facade::NodePersistence;
use crate::runtime::request_endpoints::RespondToken;
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Waker};

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "prns-grant-cancellation-{}-{id}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create test directory: {error}"),
            }
        }
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove owned test directory");
    }
}

enum Change {
    Add,
    Replace,
    Revoke,
}
enum Delivery {
    CancelLocal,
    MissingRemoteNode,
}

#[tokio::test]
async fn committed_grant_changes_survive_cancelled_callers_and_failed_remote_responses() {
    for delivery in [Delivery::CancelLocal, Delivery::MissingRemoteNode] {
        for change in [Change::Add, Change::Replace, Change::Revoke] {
            verify(&change, &delivery).await;
        }
    }
}

async fn verify(change: &Change, delivery: &Delivery) {
    let directory = Directory::new();
    let (commands, command_rx) = mpsc::unbounded_channel();
    let node = PrnsNodeHandle::over(commands);
    drop(command_rx);
    let worker = NodePersistence::custom_dir(&directory.0)
        .unwrap()
        .worker(node.clone());
    let persistence = worker.remote_control_authorization_persistence();
    let mut engine = crate::engine::EngineState::<crate::storage::GrowableHeap>::default();
    let mut remote = crate::runtime::configure_remote_control_service(
        &mut engine,
        crate::runtime::node_facade::test_remote_control_service(),
    )
    .unwrap();
    let prior = test_remote_control_grant(RemoteControlRequestKind::Describe);
    let candidate = test_remote_control_grant(RemoteControlRequestKind::AnnounceSelf);
    let administrator = RemoteControlControllerGrant::new(
        RemoteControlControllerIdentity::new(IdentityPublicKeys {
            encryption: IdentityEncryptionPublicKey::new(crate::crypto::X25519PublicKey(
                [0x75; 32],
            )),
            signing: IdentitySigningPublicKey::new(crate::crypto::Ed25519PublicKey([0x76; 32])),
        }),
        RemoteControlControllerAuthority::Administrator,
        RemoteControlRequestSet::all(),
    )
    .unwrap();
    remote.set_controller_grant(administrator).unwrap();
    if !matches!(change, Change::Add) {
        remote.set_controller_grant(prior).unwrap();
    }
    let mut snapshot =
        vec![0; crate::persistence::remote_control_controller_grants_snapshot_capacity(2)];
    let len = remote
        .write_controller_grants_snapshot(&mut snapshot)
        .unwrap()
        .unwrap();
    snapshot.truncate(len);
    persistence
        .store(
            SnapshotRegion::RemoteControlControllerGrants,
            snapshot.clone(),
        )
        .await
        .unwrap();

    let (set_completion, set_receiver) = oneshot::channel();
    let (revoke_completion, revoke_receiver) = oneshot::channel();
    let (remote_completion, remote_receiver) = oneshot::channel();
    let responder = RespondToken {
        rtt: crate::units::RttMillis::new(10),
        link_id: crate::routing::links::LinkId::new([0x77; 16]),
        request_id: crate::routing::links::request::RequestId([0x78; 16]),
    };
    let command = match (delivery, change) {
        (Delivery::CancelLocal, Change::Add | Change::Replace) => {
            RemoteControlControllerGrantCommand::SetControllerGrant {
                grant: candidate,
                completion: set_completion,
            }
        }
        (Delivery::CancelLocal, Change::Revoke) => {
            RemoteControlControllerGrantCommand::RevokeController {
                controller: *prior.controller(),
                completion: revoke_completion,
            }
        }
        (Delivery::MissingRemoteNode, Change::Add | Change::Replace) => {
            RemoteControlControllerGrantCommand::AuthorizeControllerAndRespond {
                grant: candidate,
                responder,
                node,
                completion: remote_completion,
            }
        }
        (Delivery::MissingRemoteNode, Change::Revoke) => {
            RemoteControlControllerGrantCommand::RevokeControllerAndRespond {
                controller: *prior.controller(),
                responder,
                node,
                completion: remote_completion,
            }
        }
    };
    let pause = persistence.pause_test_storage();
    let mut applying = Box::pin(command.apply(&mut remote, Some(&persistence)));
    assert!(applying
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    let mut bytes = vec![0; snapshot.len()];
    assert_eq!(
        FileStore::new(&directory.0)
            .load(SnapshotRegion::RemoteControlControllerGrants, &mut bytes)
            .unwrap(),
        Some(snapshot.as_slice())
    );
    drop(set_receiver);
    drop(revoke_receiver);
    drop(pause);
    applying.await.unwrap();
    if matches!(delivery, Delivery::MissingRemoteNode) {
        remote_receiver.await.unwrap();
    }

    let mut expected = vec![administrator];
    if !matches!(change, Change::Revoke) {
        expected.push(candidate);
    }
    expected.sort_by_key(|grant| *grant.controller().identity_hash().as_bytes());
    assert_eq!(
        remote
            .controller_grants()
            .unwrap()
            .grants_in_identity_hash_order(),
        expected.as_slice()
    );
    for _ in 0..2 {
        let store = FileStore::new(&directory.0);
        let mut bytes =
            vec![0; crate::persistence::remote_control_controller_grants_snapshot_capacity(2)];
        let loaded = store
            .load(SnapshotRegion::RemoteControlControllerGrants, &mut bytes)
            .unwrap()
            .unwrap();
        let restored: Vec<_> = read_remote_control_controller_grants_snapshot(loaded)
            .unwrap()
            .collect();
        assert_eq!(restored, expected);
    }
}
