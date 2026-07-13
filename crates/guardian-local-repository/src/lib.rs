//! Local repository adapter with isolated staging and atomic sealing.

mod error;
mod filesystem;
mod inventory;
mod repository;
mod retention;
mod signature_file;
mod staging;
mod verification;

pub use error::RepositoryError;
pub use repository::LocalRepository;
pub use retention::{RetentionOutcome, RetentionPlan};
pub use staging::{SealedBackup, StagingBackup};
