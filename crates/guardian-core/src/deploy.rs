use crate::manifest::{PayloadSelectionError, select_payloads};
use crate::{
    BackupId, Manifest, ManifestError, PayloadPath, ProfileId, RemoteTargetPath, VdsProfile,
};
use thiserror::Error;

/// A plan to push a sealed backup's payloads onto an empty/absent path on a
/// *different*, separately-enrolled, host-key-pinned `VdsProfile` — the
/// remote-deploy counterpart to `RestorePlan`'s local-path restore. See
/// `docs/adr/0007-remote-deploy-to-a-new-vds.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentPlan {
    pub backup_id: BackupId,
    pub target_profile_id: ProfileId,
    pub target_path: RemoteTargetPath,
    pub filesystem_payload: PayloadPath,
    pub database_payload: Option<PayloadPath>,
    pub confirmation: String,
}

impl DeploymentPlan {
    pub fn build(
        manifest: &Manifest,
        target_profile: &VdsProfile,
        target_path: RemoteTargetPath,
    ) -> Result<Self, DeploymentPlanError> {
        manifest
            .validate_sealed()
            .map_err(DeploymentPlanError::Manifest)?;
        target_profile
            .validate()
            .map_err(|_| DeploymentPlanError::InvalidTargetProfile)?;
        if manifest.source.profile_id == target_profile.profile_id {
            return Err(DeploymentPlanError::SameAsSourceProfile);
        }
        if manifest.source.host_key_fingerprint
            == crate::host_key_fingerprint(&target_profile.endpoint.host_pin.public_key_base64)
        {
            return Err(DeploymentPlanError::SameAsSourceHost);
        }
        let (filesystem_payload, database_payload) = select_payloads(manifest)?;
        let confirmation = format!(
            "DEPLOY {} TO {}:{}",
            manifest.backup_id.as_str(),
            target_profile.profile_id.as_str(),
            target_path.as_str()
        );
        Ok(Self {
            backup_id: manifest.backup_id.clone(),
            target_profile_id: target_profile.profile_id.clone(),
            target_path,
            filesystem_payload,
            database_payload,
            confirmation,
        })
    }

    pub fn approve(&self, confirmation: &str) -> Result<(), DeploymentPlanError> {
        (confirmation == self.confirmation)
            .then_some(())
            .ok_or(DeploymentPlanError::ConfirmationRequired)
    }
}

#[derive(Debug, Error)]
pub enum DeploymentPlanError {
    #[error("backup manifest is not a verified sealed backup")]
    Manifest(#[source] ManifestError),
    #[error("deploy target profile is invalid")]
    InvalidTargetProfile,
    #[error("deploy target profile is the same profile the backup was captured from")]
    SameAsSourceProfile,
    #[error("deploy target host key matches the backup's recorded source host")]
    SameAsSourceHost,
    #[error("backup has no supported filesystem payload")]
    NoFilesystemPayload,
    #[error("backup has more than one database payload")]
    AmbiguousDatabasePayload,
    #[error("exact deploy confirmation is required")]
    ConfirmationRequired,
}

impl From<PayloadSelectionError> for DeploymentPlanError {
    fn from(error: PayloadSelectionError) -> Self {
        match error {
            PayloadSelectionError::NoFilesystemPayload => Self::NoFilesystemPayload,
            PayloadSelectionError::AmbiguousDatabasePayload => Self::AmbiguousDatabasePayload,
        }
    }
}
