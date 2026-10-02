//! Signed Linux application slots; vendor firmware and node state are separate owners.

mod activation;
mod package;
mod storage;

pub use activation::{
    Activation, CandidateId, ExecutableDigest, Fallback, LaunchBudget, Slot, Status,
};
pub use package::{Board, Budgets, PackageError, VerifiedPackage};
pub use storage::{Appliance, Checkpoint, Error, ObserveWrites, SpaceBudget, UnobservedWrites};
