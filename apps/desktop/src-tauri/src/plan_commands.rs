use crate::ids::random_id;
use guardian_configuration::{CapturePlanStore, RepositoryStore, StoredCapturePlan};
use guardian_core::{
    BackupSelection, BackupSelectionItem, CaptureSelectionPreview, DiscoverDockerInventoryUseCase,
    FilesystemCapturePlan, PlanId, ProfileStorePort, preview_capture_selection,
};
use guardian_docker::SshDockerInventoryAdapter;
use guardian_os_keyring::OsCredentialStore;
use guardian_profile_store::ProfileStore;
use guardian_ssh::SystemOpenSsh;
use serde::Serialize;
use std::path::PathBuf;
use tauri::Manager;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanFailure {
    pub code: &'static str,
    pub message: &'static str,
    pub remediation: &'static str,
}

pub async fn preview(
    app: tauri::AppHandle,
    request: BackupSelection,
) -> Result<CaptureSelectionPreview, PlanFailure> {
    let root = app
        .path()
        .app_config_dir()
        .map_err(|_| PlanFailure::storage())?;
    tauri::async_runtime::spawn_blocking(move || preview_blocking(root, request))
        .await
        .map_err(|_| PlanFailure::internal())?
}

/// Saves the exact previewed selection as a capture plan once its confirmation matches, and
/// returns the new plan's id.
pub(crate) fn save_confirmed_selection_blocking(
    root: PathBuf,
    request: BackupSelection,
    confirmation: &str,
) -> Result<String, PlanFailure> {
    let preview = preview_blocking(root.clone(), request)?;
    if preview.confirmation != confirmation {
        return Err(PlanFailure::confirmation());
    }
    let plan_id = PlanId::parse(random_id("plan")).map_err(|_| PlanFailure::internal())?;
    let plan = FilesystemCapturePlan {
        plan_id: plan_id.clone(),
        version: 1,
        profile_id: preview.profile_id,
        repository_id: preview.repository_id,
        roots: preview
            .normalized_roots
            .iter()
            .map(|path| path.as_str().to_owned())
            .collect(),
        database_path: preview.sqlite_path.map(|path| path.as_str().to_owned()),
    };
    let stored = StoredCapturePlan::new(plan)
        .and_then(|stored| stored.with_source_layout(preview.source_layout))
        .map_err(|_| PlanFailure::invalid())?;
    CapturePlanStore::at(root.join("plans"))
        .upsert(stored)
        .map_err(|_| PlanFailure::storage())?;
    Ok(plan_id.as_str().to_owned())
}

fn preview_blocking(
    root: PathBuf,
    request: BackupSelection,
) -> Result<CaptureSelectionPreview, PlanFailure> {
    let profiles = ProfileStore::at(root.join("profiles"));
    profiles
        .get(&request.profile_id)
        .map_err(|_| PlanFailure::storage())?
        .ok_or_else(PlanFailure::invalid_reference)?;
    RepositoryStore::at(root.join("repositories"))
        .get(&request.repository_id)
        .map_err(|_| PlanFailure::storage())?
        .ok_or_else(PlanFailure::invalid_reference)?;
    let inventory = docker_inventory(&profiles, &request)?;
    preview_capture_selection(&request, inventory.as_ref()).map_err(|_| PlanFailure::invalid())
}

fn docker_inventory(
    profiles: &ProfileStore,
    request: &BackupSelection,
) -> Result<Option<guardian_core::DockerInventory>, PlanFailure> {
    if !request
        .items
        .iter()
        .any(|item| !matches!(item, BackupSelectionItem::RemotePath { .. }))
    {
        return Ok(None);
    }
    let ssh = SystemOpenSsh::default();
    DiscoverDockerInventoryUseCase {
        profiles,
        inventory: &SshDockerInventoryAdapter {
            ssh: &ssh,
            credentials: &OsCredentialStore,
        },
    }
    .execute(&request.profile_id)
    .map(Some)
    .map_err(|_| PlanFailure::invalid())
}

impl PlanFailure {
    fn invalid() -> Self {
        Self {
            code: "invalid_capture_plan",
            message: "The capture plan is invalid.",
            remediation: "Choose one server, one repository, and one or more absolute server paths without traversal.",
        }
    }
    fn invalid_reference() -> Self {
        Self {
            code: "missing_capture_plan_reference",
            message: "The selected server or repository does not exist.",
            remediation: "Refresh setup data and select an existing server and backup location.",
        }
    }
    fn confirmation() -> Self {
        Self {
            code: "capture_selection_confirmation_required",
            message: "The capture selection must be previewed and confirmed again.",
            remediation: "Review the current server selection and use its exact preview identity.",
        }
    }
    fn storage() -> Self {
        Self {
            code: "plan_storage_unavailable",
            message: "The capture plan could not be saved.",
            remediation: "Check local application storage and try again.",
        }
    }
    fn internal() -> Self {
        Self {
            code: "internal_error",
            message: "The desktop command did not complete.",
            remediation: "Try again and export redacted diagnostics if the problem persists.",
        }
    }
}
