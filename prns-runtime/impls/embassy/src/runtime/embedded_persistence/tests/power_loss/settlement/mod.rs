use super::*;
use crate::engine::{
    IssuedCommand, PrnsCommand, RemoteControlTargetPairingAuthorizationPersistence,
    RemoteControlTargetPairingFinalization, SettleRemoteControlTargetPairingAuthorization,
    Settlement,
};
use crate::remote_control::{RemoteControlControllerGrantTable, RemoteControlRequestKind};
use crate::routing::links::LinkId;
use crate::runtime::remote_control_pairing_authorizations::RemoteControlPairingAuthorizationTransactionState;
use crate::runtime::remote_control_pairing_persistence::{
    RemoteControlAuthorizationStoreExchange, RemoteControlPairingManifoldPersistence,
    RemoteControlPairingPersistenceProgress, RemoteControlPairingPersistenceRequired,
};
use crate::runtime::{CompletionPool, PrnsNodeHandle};
use embassy_futures::join::join;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};

enum Finalize {
    Complete,
    RollBack,
    HealthyRollback,
    InterruptedRollback(Cut),
}

struct Outcome {
    image: [u8; CAPACITY],
    trace: Vec<Operation>,
}

#[test]
fn successful_pairing_storage_activates_then_releases_authority() {
    verify(Finalize::Complete);
}

#[test]
fn rejected_pairing_settlement_restores_authority_and_waits_for_durable_rollback() {
    verify(Finalize::RollBack);
}

fn verify(finalize: Finalize) -> Outcome {
    embassy_futures::block_on(async {
        let commands = Channel::<CriticalSectionRawMutex, IssuedCommand, 1>::new();
        let completions = CompletionPool::<CriticalSectionRawMutex, 0>::new();
        let handle = PrnsNodeHandle::new(commands.sender(), &completions);
        let stores = RemoteControlAuthorizationStoreExchange::<CriticalSectionRawMutex>::new();
        let exchange = DiscoveryGroupConfigurationStoreExchange::new();
        let control = Rc::new(RefCell::new(Control::new()));
        let flash = Flash::boot([0xff; CAPACITY], control.clone());
        let fail_write = flash.write_fault_control();
        let policy =
            EmbeddedPersistencePolicy::hopspot_default(EmbeddedCompactionPolicy::hopspot(0));
        let mut owner = EmbeddedFlashPersistence::<_, FixedRouteSnapshotKeys<8>, _, 4, _>::with_discovery_group_store(
            flash, LAYOUT, policy, FixedRouteSnapshotKeys::new(), (|_| {}) as fn(EmbeddedPersistenceDiagnostic), &exchange,
        );
        let mut engine = EngineState::<crate::storage::GrowableHeap>::default();
        let mut remote = available_remote_control(&mut engine);
        owner
            .restore(&mut engine, &mut remote, InstantMillis(0))
            .await;
        let prior = crate::runtime::node_facade::test_remote_control_grant(
            RemoteControlRequestKind::Describe,
        );
        let candidate = crate::runtime::node_facade::test_remote_control_grant(
            RemoteControlRequestKind::AnnounceSelf,
        );
        remote.set_controller_grant(candidate).unwrap();
        let next = controller_grants_snapshot(&remote);
        remote.set_controller_grant(prior).unwrap();
        let confirmed = controller_grants_snapshot(&remote);
        assert_ne!(confirmed, next);
        assert_eq!(
            owner
                .store_remote_control_authorization_snapshot(
                    &engine,
                    RemoteControlAuthorizationSnapshotKind::ControllerGrants,
                    &confirmed,
                    InstantMillis(1)
                )
                .await,
            StoreRemoteControlAuthorizationSnapshotOutcome::Stored
        );
        let mut authorization = RemoteControlPairingAuthorizationTransactionState::new();
        let mut progress = RemoteControlPairingPersistenceProgress::new();
        let attempt_id = crate::runtime::node_facade::test_remote_control_pairing_attempt(0x92);
        progress
            .accept_required(
                RemoteControlPairingPersistenceRequired::ControllerGrant {
                    attempt_id,
                    grant: candidate,
                },
                &mut remote,
                &mut authorization,
                Some(&stores),
                handle,
            )
            .await
            .unwrap();
        assert_eq!(controller_grants_snapshot(&remote), confirmed);
        assert!(commands.receiver().try_receive().is_err());
        let mut manifold = RemoteControlPairingManifoldPersistence::new(&mut owner, &stores);
        ManifoldPersistence::<crate::storage::GrowableHeap>::deadline(&mut manifold, WRITE_TIME);
        manifold.progress(&mut engine, WRITE_TIME).await;
        let stored = stores.next_completion().await;
        assert_eq!(stored, Ok(()));
        assert_eq!(controller_grants_snapshot(&remote), confirmed);
        assert!(commands.receiver().try_receive().is_err());
        let accept = progress.accept_store_completion(
            stored,
            &mut remote,
            &mut authorization,
            &stores,
            handle,
        );
        let acknowledge = async {
            let issued = commands.receiver().receive().await;
            assert_eq!(
                issued.command,
                PrnsCommand::SettleRemoteControlTargetPairingAuthorization(
                    SettleRemoteControlTargetPairingAuthorization {
                        attempt_id,
                        persistence: RemoteControlTargetPairingAuthorizationPersistence::Persisted
                    }
                )
            );
            let finalization = match finalize {
                Finalize::Complete => {
                    RemoteControlTargetPairingFinalization::CompletionDispatched { attempt_id }
                }
                Finalize::RollBack
                | Finalize::HealthyRollback
                | Finalize::InterruptedRollback(_) => {
                    RemoteControlTargetPairingFinalization::AuthorizationRollbackRequired {
                        attempt_id,
                        retired_link: LinkId::new([0x93; 16]),
                        grant: candidate,
                    }
                }
            };
            handle.route_journaled(
                Journaled::CommandSettled {
                    id: issued.id,
                    settlement: Settlement::SettleRemoteControlTargetPairingAuthorization(Ok(
                        finalization,
                    )),
                },
                |_| panic!("settlement must reach its awaiter"),
            );
        };
        let (accepted, ()) = join(accept, acknowledge).await;
        assert_eq!(accepted, Ok(()));
        control.borrow_mut().arm(None);
        let expected = match finalize {
            Finalize::Complete => {
                assert!(progress.is_ready());
                next.clone()
            }
            Finalize::RollBack | Finalize::HealthyRollback | Finalize::InterruptedRollback(_) => {
                assert!(progress.is_waiting_for_store());
                assert_eq!(controller_grants_snapshot(&remote), confirmed);
                let mut rollback = core::pin::pin!(stores.next_completion());
                let mut context = core::task::Context::from_waker(core::task::Waker::noop());
                if let Finalize::InterruptedRollback(cut) = finalize {
                    control.borrow_mut().remove_power_at(cut);
                    ManifoldPersistence::<crate::storage::GrowableHeap>::deadline(
                        &mut manifold,
                        WRITE_TIME,
                    );
                    {
                        let mut write = core::pin::pin!(manifold.progress(&mut engine, WRITE_TIME));
                        assert!(
                            core::future::Future::poll(write.as_mut(), &mut context).is_pending()
                        );
                    }
                    assert!(control.borrow().power_removed());
                    assert!(
                        core::future::Future::poll(rollback.as_mut(), &mut context).is_pending()
                    );
                    assert!(progress.is_waiting_for_store());
                    assert_eq!(controller_grants_snapshot(&remote), confirmed);
                    drop(manifold);
                    let image = owner.journal.take().unwrap().release().into_image();
                    let trace = control.borrow().trace.clone();
                    return Outcome { image, trace };
                }
                if matches!(finalize, Finalize::RollBack) {
                    fail_write.set(true);
                    ManifoldPersistence::<crate::storage::GrowableHeap>::deadline(
                        &mut manifold,
                        WRITE_TIME,
                    );
                    manifold.progress(&mut engine, WRITE_TIME).await;
                    assert!(!fail_write.get());
                    assert!(
                        core::future::Future::poll(rollback.as_mut(), &mut context).is_pending()
                    );
                    let retry = InstantMillis(WRITE_TIME.0 + policy.retry_interval_millis);
                    ManifoldPersistence::<crate::storage::GrowableHeap>::deadline(
                        &mut manifold,
                        InstantMillis(retry.0 - 1),
                    );
                    manifold
                        .progress(&mut engine, InstantMillis(retry.0 - 1))
                        .await;
                    assert!(
                        core::future::Future::poll(rollback.as_mut(), &mut context).is_pending()
                    );
                    assert_eq!(controller_grants_snapshot(&remote), confirmed);
                    ManifoldPersistence::<crate::storage::GrowableHeap>::deadline(
                        &mut manifold,
                        retry,
                    );
                    manifold.progress(&mut engine, retry).await;
                } else {
                    ManifoldPersistence::<crate::storage::GrowableHeap>::deadline(
                        &mut manifold,
                        WRITE_TIME,
                    );
                    manifold.progress(&mut engine, WRITE_TIME).await;
                }
                assert_eq!(
                    core::future::Future::poll(rollback.as_mut(), &mut context),
                    core::task::Poll::Ready(Ok(()))
                );
                progress
                    .accept_store_completion(
                        Ok(()),
                        &mut remote,
                        &mut authorization,
                        &stores,
                        handle,
                    )
                    .await
                    .unwrap();
                assert!(progress.is_ready());
                confirmed.clone()
            }
        };
        assert_eq!(controller_grants_snapshot(&remote), expected);
        let expected_grant = match finalize {
            Finalize::Complete => candidate,
            Finalize::RollBack | Finalize::HealthyRollback | Finalize::InterruptedRollback(_) => {
                prior
            }
        };
        assert_eq!(
            remote
                .controller_grants()
                .unwrap()
                .grants_in_identity_hash_order(),
            &[expected_grant]
        );
        assert!(matches!(
            authorization,
            RemoteControlPairingAuthorizationTransactionState::Available
        ));
        drop(manifold);
        let bytes = owner.journal.take().unwrap().release().into_image();
        drop(owner);
        let mut records = Vec::new();
        let mut flash = TestFlash::new();
        flash.bytes = bytes;
        let _ = FlashJournal::open(flash, LAYOUT, &mut [0; RECORD_SCRATCH_LEN], |record| {
            if record.kind == FlashJournalRecordKind::RemoteControlControllerGrants {
                records.push(record.payload.to_vec());
            }
        })
        .await
        .unwrap();
        let expected_records = match finalize {
            Finalize::Complete => std::vec![confirmed.to_vec(), next.to_vec()],
            Finalize::RollBack | Finalize::HealthyRollback | Finalize::InterruptedRollback(_) => {
                std::vec![confirmed.to_vec(), next.to_vec(), confirmed.to_vec()]
            }
        };
        assert_eq!(records, expected_records);
        for _ in 0..2 {
            let groups = DiscoveryGroupConfigurationStoreExchange::new();
            let mut flash = TestFlash::new();
            flash.bytes = bytes;
            let mut restored = EmbeddedFlashPersistence::<_, FixedRouteSnapshotKeys<8>, _, 4, _>::with_discovery_group_store(flash, LAYOUT, policy, FixedRouteSnapshotKeys::new(), (|_| {}) as fn(EmbeddedPersistenceDiagnostic), &groups);
            let mut engine = EngineState::<crate::storage::GrowableHeap>::default();
            let mut remote = available_remote_control(&mut engine);
            restored
                .restore(&mut engine, &mut remote, InstantMillis(0))
                .await;
            assert_eq!(controller_grants_snapshot(&remote), expected);
            assert_eq!(
                remote
                    .controller_grants()
                    .unwrap()
                    .grants_in_identity_hash_order(),
                &[expected_grant]
            );
        }
        let trace = control.borrow().trace.clone();
        Outcome {
            image: bytes,
            trace,
        }
    })
}

mod interrupted;
