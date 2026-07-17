use guardian_configuration::RepositoryStore;
use guardian_core::{BackupId, CancellationHandle, JobRegistry, RepositoryId, RunId};
use guardian_local_repository::{LocalRepository, TrustedBackup};
use guardian_os_keyring::OsCredentialStore;
use guardian_signing::{PortableVerificationKey, SigningIdentityManager, VerificationIdentity};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::Manager;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreRequest {
    repository_id: String,
    backup_id: String,
    destination: String,
    confirmation: Option<String>,
    run_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorePreview {
    pub backup_id: String,
    pub destination: String,
    pub confirmation: String,
    pub payload: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreFailure {
    pub code: &'static str,
    pub message: &'static str,
    pub remediation: &'static str,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupSummary {
    pub backup_id: String,
    pub sealed_at: String,
    pub verification: &'static str,
}

impl From<&TrustedBackup> for BackupSummary {
    fn from(value: &TrustedBackup) -> Self {
        Self {
            backup_id: value.backup_id.as_str().to_owned(),
            sealed_at: value.sealed_at.as_str().to_owned(),
            verification: "verified",
        }
    }
}

pub async fn list(
    app: tauri::AppHandle,
    repository_id: String,
) -> Result<Vec<BackupSummary>, RestoreFailure> {
    let root = app
        .path()
        .app_config_dir()
        .map_err(|_| RestoreFailure::storage())?;
    tauri::async_runtime::spawn_blocking(move || list_blocking(root, repository_id))
        .await
        .map_err(|_| RestoreFailure::storage())?
}

fn list_blocking(
    root: PathBuf,
    repository_id: String,
) -> Result<Vec<BackupSummary>, RestoreFailure> {
    let (repository, identity) = resolve_repository(&root, &repository_id)?;
    repository
        .list_sealed_backups(&identity)
        .map(|inventory| inventory.iter().map(BackupSummary::from).collect())
        .map_err(|_| RestoreFailure::rejected())
}

pub async fn preview(
    app: tauri::AppHandle,
    request: RestoreRequest,
) -> Result<RestorePreview, RestoreFailure> {
    let root = app
        .path()
        .app_config_dir()
        .map_err(|_| RestoreFailure::storage())?;
    tauri::async_runtime::spawn_blocking(move || plan(root, request))
        .await
        .map_err(|_| RestoreFailure::storage())?
}

pub async fn execute(
    app: tauri::AppHandle,
    request: RestoreRequest,
) -> Result<RestorePreview, RestoreFailure> {
    let root = app
        .path()
        .app_config_dir()
        .map_err(|_| RestoreFailure::storage())?;
    let run_id = request
        .run_id
        .as_deref()
        .ok_or_else(RestoreFailure::storage)
        .and_then(|value| RunId::parse(value).map_err(|_| RestoreFailure::storage()))?;
    let handle = CancellationHandle::new();
    let registry = app.state::<JobRegistry>();
    let _registration = registry.register(run_id, handle.clone());
    tauri::async_runtime::spawn_blocking(move || execute_blocking(root, request, handle))
        .await
        .map_err(|_| RestoreFailure::storage())?
}

fn plan(root: PathBuf, request: RestoreRequest) -> Result<RestorePreview, RestoreFailure> {
    let (repository, backup_id, identity) = resolve(root, &request)?;
    let plan = repository
        .plan_restore(&backup_id, &request.destination, &identity)
        .map_err(|_| RestoreFailure::rejected())?;
    Ok(summary(plan))
}

fn execute_blocking(
    root: PathBuf,
    request: RestoreRequest,
    handle: CancellationHandle,
) -> Result<RestorePreview, RestoreFailure> {
    let confirmation = request
        .confirmation
        .as_deref()
        .ok_or_else(RestoreFailure::confirmation)?;
    let (repository, backup_id, identity) = resolve(root, &request)?;
    let result = repository.execute_restore_with_cancellation(
        &backup_id,
        &request.destination,
        confirmation,
        &identity,
        &OsCredentialStore,
        &handle,
    );
    match result {
        Ok(plan) => Ok(summary(plan)),
        Err(_) if handle.is_cancelled() => Err(RestoreFailure::cancelled()),
        Err(_) => Err(RestoreFailure::rejected()),
    }
}

fn resolve(
    root: PathBuf,
    request: &RestoreRequest,
) -> Result<(LocalRepository, BackupId, VerificationIdentity), RestoreFailure> {
    let backup_id = BackupId::parse(&request.backup_id).map_err(|_| RestoreFailure::rejected())?;
    let (repository, identity) = resolve_repository(&root, &request.repository_id)?;
    Ok((repository, backup_id, identity))
}

fn resolve_repository(
    root: &Path,
    repository_id: &str,
) -> Result<(LocalRepository, VerificationIdentity), RestoreFailure> {
    let repository_id =
        RepositoryId::parse(repository_id).map_err(|_| RestoreFailure::rejected())?;
    let registration = RepositoryStore::at(root.join("repositories"))
        .get(&repository_id)
        .map_err(|_| RestoreFailure::storage())?
        .ok_or_else(RestoreFailure::rejected)?;
    let repository = LocalRepository::open(&registration.path, repository_id)
        .map_err(|_| RestoreFailure::storage())?;
    let portable = repository
        .trusted_verification_key()
        .map_err(|_| RestoreFailure::storage())?
        .map(|key| PortableVerificationKey {
            algorithm: key.algorithm,
            key_id: key.key_id,
            public_key_base64: key.public_key_base64,
        });
    let identity = SigningIdentityManager::open(root.join("node"))
        .map_err(|_| RestoreFailure::storage())?
        .load_verifier(&OsCredentialStore, portable.as_ref())
        .map_err(|_| RestoreFailure::rejected())?;
    Ok((repository, identity))
}

fn summary(plan: guardian_core::RestorePlan) -> RestorePreview {
    RestorePreview {
        backup_id: plan.backup_id.as_str().to_owned(),
        destination: plan.destination.display().to_string(),
        confirmation: plan.confirmation,
        payload: plan.filesystem_payload.as_str().to_owned(),
    }
}

impl RestoreFailure {
    fn rejected() -> Self {
        Self {
            code: "restore_rejected",
            message: "The restore preview could not be verified safely.",
            remediation: "Use a sealed backup, a new absolute target folder, and the exact confirmation phrase.",
        }
    }
    fn confirmation() -> Self {
        Self {
            code: "restore_confirmation_required",
            message: "Exact restore confirmation is required.",
            remediation: "Copy the confirmation phrase from the preview before restoring.",
        }
    }
    fn cancelled() -> Self {
        Self {
            code: "restore_cancelled",
            message: "The restore was cancelled by the operator.",
            remediation: "The destination was not published. Review the backup and start a new restore when ready.",
        }
    }
    fn storage() -> Self {
        Self {
            code: "local_storage_unavailable",
            message: "Local application storage is unavailable.",
            remediation: "Check the repository and application storage, then try again.",
        }
    }
}
