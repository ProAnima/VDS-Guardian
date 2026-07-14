use crate::{PayloadEntry, RunId};
use thiserror::Error;

pub trait BackupStoragePort: Send + Sync {
    fn begin(&self, run_id: &RunId) -> Result<(), StoragePortError>;
    fn register_payload(&self, payload: PayloadEntry) -> Result<(), StoragePortError>;
    fn discard(&self, run_id: &RunId) -> Result<(), StoragePortError>;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StoragePortError {
    #[error("backup storage is unavailable")]
    Unavailable,
    #[error("backup storage rejected the staged payload")]
    Rejected,
}
