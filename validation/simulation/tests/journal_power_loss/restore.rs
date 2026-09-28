use super::*;
use prns_core::engine::EngineState;
use prns_core::interfaces::{
    DiscoveryGroupConfigurationEntry, DiscoveryGroupConfigurationSnapshot, DiscoveryGroupId,
    DiscoveryGroupSet, InterfaceId, DISCOVERY_GROUP_CONFIGURATION_SNAPSHOT_MAX_LEN,
    INTERFACE_ID_LEN,
};
use prns_core::persistence::TIMEBASE_HEADROOM_MILLIS;
use prns_core::remote_control::RemoteControlService;
use prns_core::storage::GrowableHeap;
use prns_core::units::InstantMillis;
use prns_runtime_embassy::runtime::{
    configure_remote_control_service, restored_discovery_group_configuration_now,
    EmbeddedCompactionPolicy, EmbeddedFlashPersistence, EmbeddedPersistenceDiagnostic,
    EmbeddedPersistencePolicy, EmbeddedPersistenceRestoreReport, FixedRouteSnapshotKeys,
};

const RAW_BOOT: InstantMillis = InstantMillis(37);
const SAVED_TIME: InstantMillis = InstantMillis(9_000);

fn snapshot(names: &[&str]) -> DiscoveryGroupConfigurationSnapshot {
    let groups: Vec<_> = names
        .iter()
        .map(|name| DiscoveryGroupId::parse(name).unwrap())
        .collect();
    DiscoveryGroupConfigurationSnapshot::try_from_entries(&[
        DiscoveryGroupConfigurationEntry::new(
            InterfaceId::new([1; INTERFACE_ID_LEN]),
            DiscoveryGroupSet::try_from_slice(&groups).unwrap(),
        ),
        DiscoveryGroupConfigurationEntry::new(
            InterfaceId::new([2; INTERFACE_ID_LEN]),
            DiscoveryGroupSet::singleton(DiscoveryGroupId::parse("wifi-only").unwrap()).unwrap(),
        ),
    ])
    .unwrap()
}

fn encode(snapshot: &DiscoveryGroupConfigurationSnapshot) -> Vec<u8> {
    let mut bytes = [0; DISCOVERY_GROUP_CONFIGURATION_SNAPSHOT_MAX_LEN];
    let len = snapshot.encode(&mut bytes).unwrap();
    bytes[..len].to_vec()
}

enum GroupRestore {
    Empty,
    Valid,
    RefusedNewest,
}

fn expected_report(
    logical_start: InstantMillis,
    groups: GroupRestore,
) -> EmbeddedPersistenceRestoreReport {
    let (restored, refused) = match groups {
        GroupRestore::Empty => (false, 0),
        GroupRestore::Valid => (true, 0),
        GroupRestore::RefusedNewest => (true, 1),
    };
    EmbeddedPersistenceRestoreReport {
        logical_start,
        route_seeded_count: 0,
        route_refused_count: 0,
        route_dropped_count: 0,
        ratchet_seeded_count: 0,
        ratchet_refused_count: 0,
        remote_control_controller_grants_restored_count: 0,
        remote_control_controller_grants_refused_count: 0,
        remote_control_controller_grants_dropped_count: 0,
        remote_control_target_accesses_restored_count: 0,
        remote_control_target_accesses_refused_count: 0,
        remote_control_target_accesses_dropped_count: 0,
        discovery_group_configuration_restored: restored,
        discovery_group_configuration_refused_count: refused,
        warning: None,
    }
}

async fn boot(
    image: Image,
) -> (
    EmbeddedPersistenceRestoreReport,
    DiscoveryGroupConfigurationSnapshot,
) {
    let mut diagnostics = Vec::new();
    let mut persistence = EmbeddedFlashPersistence::<_, FixedRouteSnapshotKeys<8>, _, 4>::new(
        Flash::boot(image),
        LAYOUT,
        EmbeddedPersistencePolicy::hopspot_default(EmbeddedCompactionPolicy::hopspot(0)),
        FixedRouteSnapshotKeys::new(),
        |event| {
            assert!(diagnostics.is_empty(), "one restore diagnostic");
            diagnostics.push(event);
        },
    );
    let mut engine = EngineState::<GrowableHeap>::default();
    let mut remote_control =
        configure_remote_control_service(&mut engine, RemoteControlService::Unavailable).unwrap();
    let report = persistence
        .restore(&mut engine, &mut remote_control, RAW_BOOT)
        .await;
    let restored = restored_discovery_group_configuration_now().unwrap();
    assert!(!persistence.state_not_saved());
    drop(persistence);
    assert_eq!(
        diagnostics,
        vec![EmbeddedPersistenceDiagnostic::Restored(report)]
    );
    (report, restored)
}

#[tokio::test]
async fn embedded_owner_restores_torn_group_updates_and_forgets_previous_boots() {
    let confirmed = snapshot(&["reticulum"]);
    let candidate = snapshot(&["field", "reticulum"]);
    let logical_start = InstantMillis(SAVED_TIME.0 + TIMEBASE_HEADROOM_MILLIS);
    let (mut journal, _, _) = open(Flash::boot(Image([0xff; CAPACITY]))).await;
    journal.initialize_empty().await.unwrap();
    journal.record_timebase(SAVED_TIME).await.unwrap();
    journal
        .append(Kind::DiscoveryGroupConfigurations, &encode(&confirmed))
        .await
        .unwrap();
    let image = journal.release().into_image();
    let expected = expected_report(logical_start, GroupRestore::Valid);
    assert_eq!(boot(image.clone()).await, (expected, confirmed));

    let (mut reference, _, _) = open(Flash::boot(image.clone())).await;
    reference
        .append(Kind::DiscoveryGroupConfigurations, &encode(&candidate))
        .await
        .unwrap();
    let reference = reference.release();
    let trace = reference.trace().to_vec();
    assert_eq!(boot(reference.into_image()).await, (expected, candidate));
    let commit = trace
        .iter()
        .rposition(|operation| matches!(operation, Operation::Write { .. }))
        .unwrap();
    assert!(matches!(trace[commit], Operation::Write { len: 4, .. }));
    let (reader, _, _) = open(Flash::boot(image.clone())).await;
    let opening = reader.release().trace().len();
    let mut cuts = 0;
    for (operation, event) in trace.iter().enumerate().skip(opening) {
        let prefixes: Vec<_> = match event {
            Operation::Read { len, .. } => vec![0, *len],
            Operation::Write { len, .. } => (0..=*len).collect(),
            Operation::Erase { .. } => unreachable!("append must not erase confirmed data"),
        };
        for completed_bytes in prefixes {
            let cut = Cut {
                operation,
                completed_bytes,
            };
            let mut flash = Flash::boot(image.clone());
            flash.arm(cut);
            let (mut journal, _, _) = open(flash).await;
            assert_eq!(
                journal
                    .append(Kind::DiscoveryGroupConfigurations, &encode(&candidate))
                    .await,
                Err(FlashJournalError::Flash(Error::PowerLost)),
                "{cut:?}"
            );
            let flash = journal.release();
            assert_eq!(flash.trace(), &trace[..=operation]);
            let committed = operation > commit || (operation == commit && completed_bytes == 4);
            let groups = if committed { candidate } else { confirmed };
            let surviving = flash.into_image();
            for _ in 0..2 {
                assert_eq!(boot(surviving.clone()).await, (expected, groups), "{cut:?}");
            }
            cuts += 1;
        }
    }

    let (mut journal, _, _) = open(Flash::boot(image)).await;
    journal
        .append(Kind::DiscoveryGroupConfigurations, &[0xff])
        .await
        .unwrap();
    assert_eq!(
        boot(journal.release().into_image()).await,
        (
            expected_report(logical_start, GroupRestore::RefusedNewest),
            confirmed
        )
    );
    assert_eq!(
        boot(Image([0xff; CAPACITY])).await,
        (
            expected_report(RAW_BOOT, GroupRestore::Empty),
            DiscoveryGroupConfigurationSnapshot::empty()
        )
    );
    eprintln!("verified {cuts} torn group-update cuts through the embedded restore owner");
}
