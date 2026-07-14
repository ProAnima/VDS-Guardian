//! Public configuration stores shared by desktop and future CLI adapters.

mod plan_store;
mod repository_store;
mod storage;

pub use plan_store::{CapturePlanStore, StoredCapturePlan};
pub use repository_store::{RepositoryRegistration, RepositoryStore};
pub use storage::ConfigurationStoreError;
