use crate::{LocalRepository, RepositoryError, StagingBackup};
use guardian_core::{BackupStoragePort, PayloadEntry, PayloadPath, RunId, StoragePortError};
use std::{path::PathBuf, sync::Mutex};

pub struct LocalRepositoryStorageAdapter<'repository> {
    repository: &'repository LocalRepository,
    staging: Mutex<Option<StagingBackup<'repository>>>,
}

impl<'repository> LocalRepositoryStorageAdapter<'repository> {
    #[must_use]
    pub fn new(repository: &'repository LocalRepository) -> Self {
        Self {
            repository,
            staging: Mutex::new(None),
        }
    }
}

impl BackupStoragePort for LocalRepositoryStorageAdapter<'_> {
    fn begin(&self, run_id: &RunId) -> Result<(), StoragePortError> {
        let mut staging = self
            .staging
            .lock()
            .map_err(|_| StoragePortError::Unavailable)?;
        if staging.is_some() {
            return Err(StoragePortError::Rejected);
        }
        *staging = Some(
            self.repository
                .begin_staging(run_id.clone())
                .map_err(map_error)?,
        );
        Ok(())
    }

    fn reserve(&self, path: &PayloadPath) -> Result<PathBuf, StoragePortError> {
        let staging = self
            .staging
            .lock()
            .map_err(|_| StoragePortError::Unavailable)?;
        staging
            .as_ref()
            .ok_or(StoragePortError::Rejected)?
            .reserve_payload_destination(path)
            .map_err(map_error)
    }

    fn register_payload_path(
        &self,
        role: &str,
        path: PayloadPath,
        media_type: &str,
    ) -> Result<PayloadEntry, StoragePortError> {
        let staging = self
            .staging
            .lock()
            .map_err(|_| StoragePortError::Unavailable)?;
        staging
            .as_ref()
            .ok_or(StoragePortError::Rejected)?
            .register_payload_file(role, path, media_type)
            .map_err(map_error)
    }

    fn discard(&self, _: &RunId) -> Result<(), StoragePortError> {
        let mut staging = self
            .staging
            .lock()
            .map_err(|_| StoragePortError::Unavailable)?;
        staging
            .take()
            .ok_or(StoragePortError::Rejected)?
            .discard()
            .map_err(map_error)
    }
}

fn map_error(_: RepositoryError) -> StoragePortError {
    StoragePortError::Rejected
}
