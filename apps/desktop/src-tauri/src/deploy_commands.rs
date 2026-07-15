use crate::job_registry::JobRegistry;
use guardian_configuration::RepositoryStore;
use guardian_core::{
    BackupId, CancellationHandle, ProfileId, ProfileStorePort, RemoteTargetPath, RepositoryId,
    RunId,
};
use guardian_deploy::DeploymentComposition;
use guardian_local_repository::LocalRepository;
use guardian_os_keyring::OsCredentialStore;
use guardian_profile_store::ProfileStore;
use guardian_signing::{ManagedIdentity, SigningIdentityManager};
use guardian_ssh::SystemOpenSsh;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::Manager;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployRequest {
    repository_id: String,
    backup_id: String,
    target_profile_id: String,
    target_path: String,
    confirmation: Option<String>,
    run_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentPreview {
    pub backup_id: String,
    pub target_profile_id: String,
    pub target_profile_label: String,
    pub target_path: String,
    pub confirmation: String,
    pub filesystem_payload: String,
    pub database_payload: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployFailure {
    pub code: &'static str,
    pub message: &'static str,
    pub remediation: &'static str,
}

pub async fn preview(
    app: tauri::AppHandle,
    request: DeployRequest,
) -> Result<DeploymentPreview, DeployFailure> {
    let root = app
        .path()
        .app_config_dir()
        .map_err(|_| DeployFailure::storage())?;
    tauri::async_runtime::spawn_blocking(move || plan_blocking(root, request))
        .await
        .map_err(|_| DeployFailure::storage())?
}

pub async fn execute(
    app: tauri::AppHandle,
    request: DeployRequest,
) -> Result<DeploymentPreview, DeployFailure> {
    let root = app
        .path()
        .app_config_dir()
        .map_err(|_| DeployFailure::storage())?;
    let run_id = request
        .run_id
        .as_deref()
        .ok_or_else(DeployFailure::storage)
        .and_then(|value| RunId::parse(value).map_err(|_| DeployFailure::storage()))?;
    let handle = CancellationHandle::new();
    // Registered before `spawn_blocking`, before the run_id moves into the
    // blocking closure, so a concurrent `cancel_job` call can find it while
    // this deploy is still in flight.
    let registry = app.state::<JobRegistry>();
    let _registration = registry.register(run_id.clone(), handle.clone());
    tauri::async_runtime::spawn_blocking(move || execute_blocking(root, request, run_id, handle))
        .await
        .map_err(|_| DeployFailure::storage())?
}

fn plan_blocking(
    root: PathBuf,
    request: DeployRequest,
) -> Result<DeploymentPreview, DeployFailure> {
    let (inputs, backup_id, target_path) = resolve(&root, &request)?;
    let ssh = SystemOpenSsh::default();
    let composition = DeploymentComposition {
        repository: &inputs.repository,
        ssh: &ssh,
        target_profile: &inputs.target_profile,
        credentials: &OsCredentialStore,
        verifier: &inputs.identity,
    };
    let plan = composition
        .plan(&backup_id, target_path)
        .map_err(|_| DeployFailure::rejected())?;
    Ok(summary(plan, &inputs.target_profile))
}

fn execute_blocking(
    root: PathBuf,
    request: DeployRequest,
    run_id: RunId,
    handle: CancellationHandle,
) -> Result<DeploymentPreview, DeployFailure> {
    let confirmation = request
        .confirmation
        .as_deref()
        .ok_or_else(DeployFailure::confirmation)?;
    let (inputs, backup_id, target_path) = resolve(&root, &request)?;
    let target_profile_id = inputs.target_profile.profile_id.clone();
    let ssh = SystemOpenSsh::default().with_cancellation(handle.clone());
    let composition = DeploymentComposition {
        repository: &inputs.repository,
        ssh: &ssh,
        target_profile: &inputs.target_profile,
        credentials: &OsCredentialStore,
        verifier: &inputs.identity,
    };
    inputs
        .repository
        .write_deploy_audit(&run_id, "attempted", &backup_id, &target_profile_id)
        .map_err(|_| DeployFailure::storage())?;
    match composition.execute(&target_profile_id, &backup_id, target_path, confirmation) {
        Ok(plan) => {
            inputs
                .repository
                .write_deploy_audit(&run_id, "completed", &backup_id, &target_profile_id)
                .map_err(|_| DeployFailure::storage())?;
            Ok(summary(plan, &inputs.target_profile))
        }
        Err(_) if handle.is_cancelled() => {
            let _ = inputs.repository.write_deploy_audit(
                &run_id,
                "cancelled",
                &backup_id,
                &target_profile_id,
            );
            Err(DeployFailure::cancelled())
        }
        Err(_) => {
            let _ = inputs.repository.write_deploy_audit(
                &run_id,
                "failed",
                &backup_id,
                &target_profile_id,
            );
            Err(DeployFailure::rejected())
        }
    }
}

struct ResolvedDeployInputs {
    repository: LocalRepository,
    target_profile: guardian_core::VdsProfile,
    identity: ManagedIdentity,
}

fn resolve(
    root: &Path,
    request: &DeployRequest,
) -> Result<(ResolvedDeployInputs, BackupId, RemoteTargetPath), DeployFailure> {
    let repository_id =
        RepositoryId::parse(&request.repository_id).map_err(|_| DeployFailure::rejected())?;
    let registration = RepositoryStore::at(root.join("repositories"))
        .get(&repository_id)
        .map_err(|_| DeployFailure::storage())?
        .ok_or_else(DeployFailure::rejected)?;
    let repository = LocalRepository::open(&registration.path, repository_id)
        .map_err(|_| DeployFailure::storage())?;
    let identity = SigningIdentityManager::open(root.join("node"))
        .map_err(|_| DeployFailure::storage())?
        .load_ready(&OsCredentialStore)
        .map_err(|_| DeployFailure::rejected())?;
    let backup_id = BackupId::parse(&request.backup_id).map_err(|_| DeployFailure::rejected())?;
    let target_profile_id =
        ProfileId::parse(&request.target_profile_id).map_err(|_| DeployFailure::rejected())?;
    let target_profile = ProfileStore::at(root.join("profiles"))
        .get(&target_profile_id)
        .map_err(|_| DeployFailure::storage())?
        .ok_or_else(DeployFailure::rejected)?;
    let target_path =
        RemoteTargetPath::parse(&request.target_path).map_err(|_| DeployFailure::rejected())?;
    Ok((
        ResolvedDeployInputs {
            repository,
            target_profile,
            identity,
        },
        backup_id,
        target_path,
    ))
}

fn summary(
    plan: guardian_core::DeploymentPlan,
    target_profile: &guardian_core::VdsProfile,
) -> DeploymentPreview {
    DeploymentPreview {
        backup_id: plan.backup_id.as_str().to_owned(),
        target_profile_id: plan.target_profile_id.as_str().to_owned(),
        target_profile_label: target_profile.label.clone(),
        target_path: plan.target_path.as_str().to_owned(),
        confirmation: plan.confirmation,
        filesystem_payload: plan.filesystem_payload.as_str().to_owned(),
        database_payload: plan
            .database_payload
            .as_ref()
            .map(|path| path.as_str().to_owned()),
    }
}

impl DeployFailure {
    fn rejected() -> Self {
        Self {
            code: "deploy_rejected",
            message: "The deploy could not be verified or pushed safely.",
            remediation: "Use a sealed backup, a different already-enrolled target server, an absolute path that does not exist yet, and the exact confirmation phrase.",
        }
    }
    fn confirmation() -> Self {
        Self {
            code: "deploy_confirmation_required",
            message: "Exact deploy confirmation is required.",
            remediation: "Copy the confirmation phrase from the preview before deploying.",
        }
    }
    fn cancelled() -> Self {
        Self {
            code: "deploy_cancelled",
            message: "The deploy was cancelled by the operator.",
            remediation: "Start a new deploy if it should still happen.",
        }
    }
    fn storage() -> Self {
        Self {
            code: "local_storage_unavailable",
            message: "Local application storage is unavailable.",
            remediation: "Check the repository, target server enrollment, and application storage, then try again.",
        }
    }
}
