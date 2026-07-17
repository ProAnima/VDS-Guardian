use crate::manifest::{PayloadSelectionError, select_payloads};
use crate::{BackupId, Manifest, ManifestError, PayloadPath};
use serde::{Deserialize, Serialize};
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreMode {
    NewDestination,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreImpactPreview {
    pub backup_id: BackupId,
    pub destination: PathBuf,
    pub mode: RestoreMode,
    pub adds: Vec<PathBuf>,
    pub replaces: Vec<PathBuf>,
    pub conflicts: Vec<PathBuf>,
    pub workload_labels: Vec<String>,
    pub confirmation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestorePlan {
    pub backup_id: BackupId,
    pub destination: PathBuf,
    pub filesystem_payload: PayloadPath,
    pub database_payload: Option<PayloadPath>,
    pub confirmation: String,
    pub impact: RestoreImpactPreview,
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
        let (filesystem_payload, database_payload) = select_payloads(manifest)?;
        let confirmation = format!(
            "RESTORE {} TO {}",
            manifest.backup_id.as_str(),
            destination.display()
        );
        let mut adds = vec![destination.clone()];
        let mut workload_labels = vec!["filesystem".to_owned()];
        if database_payload.is_some() {
            adds.push(destination.join("database.sqlite"));
            workload_labels.push("sqlite".to_owned());
        }
        let conflicts = destination_occupied(&destination)
            .then(|| destination.clone())
            .into_iter()
            .collect();
        let impact = RestoreImpactPreview {
            backup_id: manifest.backup_id.clone(),
            destination: destination.clone(),
            mode: RestoreMode::NewDestination,
            adds,
            replaces: Vec::new(),
            conflicts,
            workload_labels,
            confirmation: confirmation.clone(),
        };
        Ok(Self {
            backup_id: manifest.backup_id.clone(),
            destination,
            filesystem_payload,
            database_payload,
            confirmation,
            impact,
        })
    }

    pub fn approve(&self, confirmation: &str) -> Result<(), RestorePlanError> {
        if confirmation != self.confirmation {
            return Err(RestorePlanError::ConfirmationRequired);
        }
        if !self.impact.conflicts.is_empty() {
            return Err(RestorePlanError::ConflictsPresent);
        }
        Ok(())
    }

    #[must_use]
    pub fn destination_is_new(&self) -> bool {
        !destination_occupied(Path::new(&self.destination))
    }
}

fn destination_occupied(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(error) if error.kind() == ErrorKind::NotFound => false,
        Err(_) => true,
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
    #[error("backup has more than one database payload")]
    AmbiguousDatabasePayload,
    #[error("exact restore confirmation is required")]
    ConfirmationRequired,
    #[error("restore impact contains conflicts that prevent safe execution")]
    ConflictsPresent,
}

impl From<PayloadSelectionError> for RestorePlanError {
    fn from(error: PayloadSelectionError) -> Self {
        match error {
            PayloadSelectionError::NoFilesystemPayload => Self::NoFilesystemPayload,
            PayloadSelectionError::AmbiguousDatabasePayload => Self::AmbiguousDatabasePayload,
        }
    }
}
