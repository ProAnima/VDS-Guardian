use crate::{BackupId, Manifest, ManifestSigner, PayloadEntry, PayloadPath, RunId, Timestamp};
use std::path::PathBuf;
use thiserror::Error;

pub trait BackupStoragePort: Send + Sync {
    fn begin(&self, run_id: &RunId) -> Result<(), StoragePortError>;
    fn reserve(&self, path: &PayloadPath) -> Result<PathBuf, StoragePortError>;
    fn register_payload_path(
        &self,
        role: &str,
        path: PayloadPath,
        media_type: &str,
    ) -> Result<PayloadEntry, StoragePortError>;
    fn seal(
        &self,
        manifest: Manifest,
        sealed_at: Timestamp,
        signer: &dyn ManifestSigner,
    ) -> Result<SealedBackup, StoragePortError>;
    fn discard(&self, run_id: &RunId) -> Result<(), StoragePortError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedBackup {
    pub backup_id: BackupId,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StoragePortError {
    #[error("backup storage is unavailable")]
    Unavailable,
    #[error("backup storage rejected the staged payload")]
    Rejected,
}
