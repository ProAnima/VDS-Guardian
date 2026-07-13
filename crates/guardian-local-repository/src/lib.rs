//! Local repository adapter with isolated staging and atomic sealing.

mod error;
mod filesystem;
mod repository;
mod staging;
mod verification;

pub use error::RepositoryError;
pub use repository::LocalRepository;
pub use staging::{SealedBackup, StagingBackup};
