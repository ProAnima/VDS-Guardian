//! Local repository adapter with isolated staging and atomic sealing.

mod core_adapter;
mod error;
mod filesystem;
mod inventory;
mod process_lock;
mod recovery_bundle;
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
pub use inventory::TrustedBackup;
pub use recovery_bundle::{
    RecoveryBundleError, export_confirmation_phrase, export_recovery_bundle,
    import_confirmation_phrase, import_recovery_bundle,
};
pub use repository::{LocalRepository, RepositoryVerificationKey};
pub use staging::{SealedBackup, StagingBackup};
