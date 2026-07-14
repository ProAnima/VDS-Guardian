//! Local repository adapter with isolated staging and atomic sealing.

mod core_adapter;
mod error;
mod filesystem;
mod inventory;
mod process_lock;
mod repository;
mod retention;
// Intentionally cohesive: the crash-recovery state machine is reviewed as one
// protocol, despite exceeding the usual module-size target.
mod retention_journal;
mod signature_file;
mod staging;
mod verification;

pub use core_adapter::LocalRepositoryStorageAdapter;
pub use error::RepositoryError;
pub use guardian_core::{RetentionOutcome, RetentionPlan};
pub use repository::LocalRepository;
pub use staging::{SealedBackup, StagingBackup};
