use guardian_configuration::RepositoryStore;
use guardian_core::{BackupId, RepositoryId};
use guardian_local_repository::LocalRepository;
use guardian_os_keyring::OsCredentialStore;
use guardian_signing::SigningIdentityManager;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Manager;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreRequest {
    repository_id: String,
    backup_id: String,
    destination: String,
    confirmation: Option<String>,
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
    tauri::async_runtime::spawn_blocking(move || execute_blocking(root, request))
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
) -> Result<RestorePreview, RestoreFailure> {
    let confirmation = request
        .confirmation
        .as_deref()
        .ok_or_else(RestoreFailure::confirmation)?;
    let (repository, backup_id, identity) = resolve(root, &request)?;
    let plan = repository
        .execute_restore(&backup_id, &request.destination, confirmation, &identity)
        .map_err(|_| RestoreFailure::rejected())?;
    Ok(summary(plan))
}

fn resolve(
    root: PathBuf,
    request: &RestoreRequest,
) -> Result<(LocalRepository, BackupId, guardian_signing::ManagedIdentity), RestoreFailure> {
    let repository_id =
        RepositoryId::parse(&request.repository_id).map_err(|_| RestoreFailure::rejected())?;
    let backup_id = BackupId::parse(&request.backup_id).map_err(|_| RestoreFailure::rejected())?;
    let registration = RepositoryStore::at(root.join("repositories"))
        .get(&repository_id)
        .map_err(|_| RestoreFailure::storage())?
        .ok_or_else(RestoreFailure::rejected)?;
    let repository = LocalRepository::open(&registration.path, repository_id)
        .map_err(|_| RestoreFailure::storage())?;
    let identity = SigningIdentityManager::open(root.join("node"))
        .map_err(|_| RestoreFailure::storage())?
        .load_ready(&OsCredentialStore)
        .map_err(|_| RestoreFailure::rejected())?;
    Ok((repository, backup_id, identity))
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
    fn storage() -> Self {
        Self {
            code: "local_storage_unavailable",
            message: "Local application storage is unavailable.",
            remediation: "Check the repository and application storage, then try again.",
        }
    }
}
