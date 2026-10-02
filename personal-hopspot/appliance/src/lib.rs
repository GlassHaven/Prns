//! Signed Linux application slots; vendor firmware and node state are separate owners.

mod activation;
mod package;
mod radio;
mod storage;

pub use activation::{
    Activation, CandidateId, ExecutableDigest, Fallback, LaunchBudget, Slot, Status,
};
pub use package::{Board, Budgets, PackageError, VerifiedPackage};
pub use radio::{
    MeshId, MeshPathSetup, RadioBinding, RadioDevice, RadioPlan, RadioPreset, RadioProfile,
    RadioProfileError, RegionalChannel, UciSection,
};
pub use storage::{Appliance, Checkpoint, Error, ObserveWrites, SpaceBudget, UnobservedWrites};
