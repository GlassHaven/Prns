use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use personal_rns::persistence::{
    FileStore, FileStoreConfirmation, FileStoreError, PersistedStore, SnapshotRegion,
};
use personal_rns::runtime::{
    PersistenceIo, PersistenceIoCompletion, PersistenceIoError, PersistenceIoOperation,
    PersistenceIoTask,
};
use tokio::sync::oneshot;

const MAX_IO_EVENTS: usize = 4096;
static DIRECTORY_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    WriteFailed,
    PublishedUnconfirmed,
    ConfirmationFailed,
    ConfirmationMissing,
    ConfirmationDifferent,
}

#[derive(Debug)]
enum InjectedFailure {
    Write,
    Publication,
    Confirmation,
}

impl std::fmt::Display for InjectedFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for InjectedFailure {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IoEvent {
    Started(PersistenceIoOperation),
    Finished(PersistenceIoOperation),
    Effect {
        region: SnapshotRegion,
        effect: Effect,
    },
}

struct Gate {
    operation: PersistenceIoOperation,
    release: oneshot::Receiver<()>,
}

#[derive(Default)]
struct State {
    events: Vec<IoEvent>,
    gates: Vec<Gate>,
    effects: VecDeque<(SnapshotRegion, Effect)>,
}

#[derive(Clone, Default)]
pub struct ControlledIo(Arc<Mutex<State>>);

impl ControlledIo {
    pub fn gate(&self, operation: PersistenceIoOperation) -> oneshot::Sender<()> {
        let (release, ready) = oneshot::channel();
        self.0.lock().expect("scenario I/O state").gates.push(Gate {
            operation,
            release: ready,
        });
        release
    }

    pub fn script(&self, region: SnapshotRegion, effects: impl IntoIterator<Item = Effect>) {
        self.0
            .lock()
            .expect("scenario I/O state")
            .effects
            .extend(effects.into_iter().map(|effect| (region, effect)));
    }

    pub fn events(&self) -> Vec<IoEvent> {
        self.0.lock().expect("scenario I/O state").events.clone()
    }

    pub fn remaining_effects(&self) -> usize {
        self.0.lock().expect("scenario I/O state").effects.len()
    }

    fn effect(&self, region: SnapshotRegion) -> Option<Effect> {
        let mut state = self.0.lock().expect("scenario I/O state");
        let (expected, _) = state.effects.front()?;
        assert_eq!(*expected, region, "script belongs to this snapshot owner");
        let (_, effect) = state.effects.pop_front().expect("scripted effect");
        Self::record(
            &mut state,
            IoEvent::Effect {
                region,
                effect: effect.clone(),
            },
        );
        Some(effect)
    }

    fn record(state: &mut State, event: IoEvent) {
        assert!(
            state.events.len() < MAX_IO_EVENTS,
            "bounded persistence trace"
        );
        state.events.push(event);
    }
}

impl PersistenceIo for ControlledIo {
    fn execute(&self, task: PersistenceIoTask) -> PersistenceIoCompletion<'_> {
        Box::pin(async move {
            let operation = task.operation().clone();
            let gate = {
                let mut state = self.0.lock().expect("scenario I/O state");
                Self::record(&mut state, IoEvent::Started(operation.clone()));
                state
                    .gates
                    .iter()
                    .position(|gate| gate.operation == operation)
                    .map(|index| state.gates.remove(index))
            };
            if let Some(gate) = gate {
                gate.release
                    .await
                    .map_err(|_| PersistenceIoError::WorkerStopped)?;
            }
            task.run();
            Self::record(
                &mut self.0.lock().expect("scenario I/O state"),
                IoEvent::Finished(operation),
            );
            Ok(())
        })
    }

    fn store_authorization(
        &self,
        store: &mut FileStore,
        region: SnapshotRegion,
        snapshot: &[u8],
    ) -> Result<(), FileStoreError> {
        match self.effect(region) {
            None => store.store(region, snapshot),
            Some(Effect::WriteFailed) => Err(FileStoreError::Io(std::io::Error::other(
                InjectedFailure::Write,
            ))),
            Some(Effect::PublishedUnconfirmed) => {
                store.store(region, snapshot)?;
                Err(FileStoreError::PublishedDurabilityUnconfirmed(
                    std::io::Error::other(InjectedFailure::Publication),
                ))
            }
            Some(effect) => unreachable!("confirmation effect used during store: {effect:?}"),
        }
    }

    fn confirm_authorization(
        &self,
        store: &FileStore,
        region: SnapshotRegion,
        snapshot: &[u8],
    ) -> Result<FileStoreConfirmation, FileStoreError> {
        match self.effect(region) {
            None => store.confirm_store(region, snapshot),
            Some(Effect::ConfirmationFailed) => Err(FileStoreError::Io(std::io::Error::other(
                InjectedFailure::Confirmation,
            ))),
            Some(Effect::ConfirmationMissing) => Ok(FileStoreConfirmation::Missing),
            Some(Effect::ConfirmationDifferent) => Ok(FileStoreConfirmation::Different),
            Some(effect) => unreachable!("write effect used during confirmation: {effect:?}"),
        }
    }
}

struct Directory(PathBuf);

impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove scenario storage");
    }
}

#[derive(Clone)]
pub struct Storage {
    directory: Arc<Directory>,
    pub io: ControlledIo,
}

impl Storage {
    pub fn new() -> Self {
        let sequence = DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("prns-rc-sim-{}-{sequence}", std::process::id()));
        std::fs::create_dir(&directory).expect("isolated scenario storage");
        Self {
            directory: Arc::new(Directory(directory)),
            io: ControlledIo::default(),
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory.0
    }

    pub fn grants(&self) -> Vec<personal_rns::remote_control::RemoteControlControllerGrant> {
        let store = FileStore::new(self.directory());
        let region = SnapshotRegion::RemoteControlControllerGrants;
        let Some(len) = store.stored_len(region).expect("read region length") else {
            return Vec::new();
        };
        let mut bytes = vec![0; len];
        let bytes = store
            .load(region, &mut bytes)
            .expect("load sealed grants")
            .expect("stored grants");
        personal_rns::persistence::read_remote_control_controller_grants_snapshot(bytes)
            .expect("valid grant snapshot")
            .collect()
    }
}
