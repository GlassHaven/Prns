use super::*;

mod flash;
use flash::{Control, Cut, Flash, Operation};

const WRITE_TIME: InstantMillis = InstantMillis(100);
const MAX_PROGRESS_STEPS: usize = 32;

#[derive(Clone, Copy, Debug)]
enum Campaign {
    Append,
    CompactThenAppend,
}

async fn baseline(campaign: Campaign) -> [u8; CAPACITY] {
    let (mut journal, _) = FlashJournal::open(
        TestFlash::new(),
        LAYOUT,
        &mut [0; RECORD_SCRATCH_LEN],
        |_| {},
    )
    .await
    .unwrap();
    journal.initialize_empty().await.unwrap();
    let mut payload = [0; DISCOVERY_GROUP_CONFIGURATION_SNAPSHOT_MAX_LEN];
    let len = discovery_group_snapshot("confirmed").encode_into_max(&mut payload);
    journal
        .append(
            FlashJournalRecordKind::DiscoveryGroupConfigurations,
            &payload[..len],
        )
        .await
        .unwrap();
    if matches!(campaign, Campaign::CompactThenAppend) {
        let mut full = false;
        for _ in 0..32 {
            match journal
                .append(
                    FlashJournalRecordKind::DiscoveryGroupConfigurations,
                    &payload[..len],
                )
                .await
            {
                Ok(()) => {}
                Err(FlashJournalError::ArenaFull) => {
                    full = true;
                    break;
                }
                Err(error) => panic!("unexpected baseline error: {error:?}"),
            }
        }
        assert!(full, "bounded arena fill");
    }
    journal.release().bytes
}

struct Outcome {
    image: [u8; CAPACITY],
    trace: Vec<Operation>,
    completion: Result<(), EmbeddedPersistenceFailure>,
    published: DiscoveryGroupConfigurationSnapshot,
}

async fn write(image: [u8; CAPACITY], cut: Option<Cut>) -> Outcome {
    let exchange = DiscoveryGroupConfigurationStoreExchange::new();
    let control = Rc::new(RefCell::new(Control::new()));
    let mut owner = EmbeddedFlashPersistence::<_, FixedRouteSnapshotKeys<8>, _, 4, _>::with_discovery_group_store(
        Flash::boot(image, control.clone()), LAYOUT,
        EmbeddedPersistencePolicy::hopspot_default(EmbeddedCompactionPolicy::hopspot(0)),
        FixedRouteSnapshotKeys::new(), (|_| {}) as fn(EmbeddedPersistenceDiagnostic), &exchange,
    );
    let mut engine = EngineState::<crate::storage::GrowableHeap>::default();
    let mut remote = available_remote_control(&mut engine);
    owner
        .restore(&mut engine, &mut remote, InstantMillis(0))
        .await;
    assert_eq!(
        exchange.restored_now(),
        Some(discovery_group_snapshot("confirmed"))
    );
    control.borrow_mut().arm(cut);
    let interface =
        crate::interfaces::InterfaceId::new([0x42; crate::interfaces::INTERFACE_ID_LEN]);
    let candidate = discovery_group_snapshot("candidate");
    let change = DiscoveryGroupConfigurationChange::upsert(
        interface,
        *candidate.groups_for(interface).unwrap(),
    );
    let mut completion = core::pin::pin!(exchange.store(change));
    let mut context = core::task::Context::from_waker(core::task::Waker::noop());
    assert!(core::future::Future::poll(completion.as_mut(), &mut context).is_pending());
    let mut result = None;
    for _ in 0..MAX_PROGRESS_STEPS {
        owner.progress(&mut engine, WRITE_TIME).await;
        if let core::task::Poll::Ready(value) =
            core::future::Future::poll(completion.as_mut(), &mut context)
        {
            result = Some(value);
            break;
        }
    }
    let completion = result.expect("bounded persistence settlement");
    let published = exchange.restored_now().unwrap();
    let image = owner.journal.take().unwrap().release().into_image();
    let trace = control.borrow().trace.clone();
    Outcome {
        image,
        trace,
        completion,
        published,
    }
}

async fn reboot(image: [u8; CAPACITY]) -> DiscoveryGroupConfigurationSnapshot {
    let exchange = DiscoveryGroupConfigurationStoreExchange::new();
    let mut flash = TestFlash::new();
    flash.bytes = image;
    let mut owner = EmbeddedFlashPersistence::<_, FixedRouteSnapshotKeys<8>, _, 4, _>::with_discovery_group_store(
        flash, LAYOUT,
        EmbeddedPersistencePolicy::hopspot_default(EmbeddedCompactionPolicy::hopspot(0)),
        FixedRouteSnapshotKeys::new(), (|_| {}) as fn(EmbeddedPersistenceDiagnostic), &exchange,
    );
    let mut engine = EngineState::<crate::storage::GrowableHeap>::default();
    let mut remote = available_remote_control(&mut engine);
    let report = owner
        .restore(&mut engine, &mut remote, InstantMillis(0))
        .await;
    assert!(report.discovery_group_configuration_restored);
    assert_eq!(report.discovery_group_configuration_refused_count, 0);
    assert!(!owner.state_not_saved());
    exchange.restored_now().unwrap()
}

#[test]
fn queued_group_append_recovers_at_every_torn_io_boundary() {
    campaign(Campaign::Append);
}

#[test]
fn queued_group_compaction_recovers_at_every_torn_io_boundary() {
    campaign(Campaign::CompactThenAppend);
}

fn campaign(campaign: Campaign) {
    embassy_futures::block_on(async {
        let image = baseline(campaign).await;
        let reference = write(image, None).await;
        assert_eq!(
            reference
                .trace
                .iter()
                .any(|op| matches!(op, Operation::Erase { .. })),
            matches!(campaign, Campaign::CompactThenAppend)
        );
        let confirmed = discovery_group_snapshot("confirmed");
        let candidate = discovery_group_snapshot("candidate");
        assert_eq!(reference.completion, Ok(()));
        assert_eq!(reference.published, candidate);
        assert_eq!(reboot(reference.image).await, candidate);
        let commit = reference
            .trace
            .iter()
            .rposition(|op| matches!(op, Operation::Write { .. }))
            .unwrap();
        assert!(matches!(
            reference.trace[commit],
            Operation::Write { len: 4, .. }
        ));
        let mut cuts = 0;
        for (operation, event) in reference.trace.iter().enumerate() {
            let prefixes: Vec<_> = match event {
                Operation::Read { len, .. } => std::vec![0, *len],
                Operation::Write { len, .. } => (0..=*len).collect(),
                Operation::Erase { len, .. } => (0..=*len).collect(),
            };
            for completed_bytes in prefixes {
                let cut = Cut {
                    operation,
                    completed_bytes,
                };
                let outcome = write(image, Some(cut)).await;
                assert_eq!(outcome.trace, reference.trace[..=operation], "{cut:?}");
                assert_eq!(
                    outcome.completion,
                    Err(EmbeddedPersistenceFailure::Flash),
                    "{cut:?}"
                );
                assert_eq!(outcome.published, confirmed, "{cut:?}");
                let durable = if operation > commit || (operation == commit && completed_bytes == 4)
                {
                    candidate
                } else {
                    confirmed
                };
                for _ in 0..2 {
                    assert_eq!(reboot(outcome.image).await, durable, "{cut:?}");
                }
                cuts += 1;
            }
        }
        std::eprintln!(
            "verified {cuts} queued-owner {campaign:?} cuts and repeated durable recovery"
        );
    });
}
