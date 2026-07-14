use crate::{BackupId, Manifest, ManifestError, PayloadPath};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestorePlan {
    pub backup_id: BackupId,
    pub destination: PathBuf,
    pub filesystem_payload: PayloadPath,
    pub confirmation: String,
}

impl RestorePlan {
    pub fn build(
        manifest: &Manifest,
        destination: impl Into<PathBuf>,
    ) -> Result<Self, RestorePlanError> {
        manifest
            .validate_sealed()
            .map_err(RestorePlanError::Manifest)?;
        let destination = destination.into();
        if !destination.is_absolute() {
            return Err(RestorePlanError::UnsafeDestination);
        }
        let mut filesystem_payloads = manifest
            .payloads
            .iter()
            .filter(|payload| payload.media_type == "application/zstd")
            .map(|payload| payload.path.clone())
            .collect::<Vec<_>>();
        if filesystem_payloads.len() != 1 {
            return Err(RestorePlanError::NoFilesystemPayload);
        }
        let confirmation = format!(
            "RESTORE {} TO {}",
            manifest.backup_id.as_str(),
            destination.display()
        );
        Ok(Self {
            backup_id: manifest.backup_id.clone(),
            destination,
            filesystem_payload: filesystem_payloads.remove(0),
            confirmation,
        })
    }

    pub fn approve(&self, confirmation: &str) -> Result<(), RestorePlanError> {
        (confirmation == self.confirmation)
            .then_some(())
            .ok_or(RestorePlanError::ConfirmationRequired)
    }

    #[must_use]
    pub fn destination_is_new(&self) -> bool {
        !Path::new(&self.destination).exists()
    }
}

#[derive(Debug, Error)]
pub enum RestorePlanError {
    #[error("backup manifest is not a verified sealed backup")]
    Manifest(#[source] ManifestError),
    #[error("restore destination must be absolute")]
    UnsafeDestination,
    #[error("backup has no supported filesystem payload")]
    NoFilesystemPayload,
    #[error("exact restore confirmation is required")]
    ConfirmationRequired,
}
