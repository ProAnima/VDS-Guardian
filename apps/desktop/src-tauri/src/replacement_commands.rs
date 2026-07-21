use crate::{job_commands, restore_commands};
use guardian_configuration::CapturePlanStore;
use guardian_core::{
    BackupId, CancellationHandle, JobRegistry, ProfileId, ProfileStorePort, RepositoryId, RunId,
    SourceReplacementImpact,
};
use guardian_deploy::ReplacementComposition;
use guardian_docker::SshDockerInventoryAdapter;
use guardian_os_keyring::OsCredentialStore;
use guardian_profile_store::ProfileStore;
use guardian_ssh::SystemOpenSsh;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::Manager;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplacementRequest {
    repository_id: String,
    backup_id: String,
    target_profile_id: String,
    confirmation: Option<String>,
    run_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplacementResult {
    #[serde(flatten)]
    pub impact: SourceReplacementImpact,
    pub safety_backup_id: Option<String>,
    pub rollback_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplacementFailure {
    pub code: &'static str,
    pub message: &'static str,
    pub remediation: &'static str,
}

pub async fn preview(
    app: tauri::AppHandle,
    request: ReplacementRequest,
) -> Result<ReplacementResult, ReplacementFailure> {
    let root = app
        .path()
        .app_config_dir()
        .map_err(|_| ReplacementFailure::storage())?;
    tauri::async_runtime::spawn_blocking(move || plan_blocking(&root, &request))
        .await
        .map_err(|_| ReplacementFailure::storage())?
}

pub async fn execute(
    app: tauri::AppHandle,
    request: ReplacementRequest,
) -> Result<ReplacementResult, ReplacementFailure> {
    let root = app
        .path()
        .app_config_dir()
        .map_err(|_| ReplacementFailure::storage())?;
    let run_id = parse_run_id(&request)?;
    let handle = CancellationHandle::new();
    let registry = app.state::<JobRegistry>();
    let _registration = registry.register(run_id.clone(), handle.clone());
    tauri::async_runtime::spawn_blocking(move || execute_blocking(root, request, run_id, handle))
        .await
        .map_err(|_| ReplacementFailure::storage())?
}

fn plan_blocking(
    root: &Path,
    request: &ReplacementRequest,
) -> Result<ReplacementResult, ReplacementFailure> {
    let inputs = resolve(root, request)?;
    let ssh = SystemOpenSsh::default();
    let docker = SshDockerInventoryAdapter {
        ssh: &ssh,
        credentials: &OsCredentialStore,
    };
    let composition = composition(&inputs, &ssh, &docker);
    let plan = composition
        .plan(&inputs.backup_id)
        .map_err(|_| ReplacementFailure::rejected())?;
    Ok(result(plan.impact, None, "pending"))
}

fn execute_blocking(
    root: PathBuf,
    request: ReplacementRequest,
    run_id: RunId,
    handle: CancellationHandle,
) -> Result<ReplacementResult, ReplacementFailure> {
    let confirmation = request
        .confirmation
        .as_deref()
        .ok_or_else(ReplacementFailure::confirmation)?;
    let ssh = SystemOpenSsh::default().with_cancellation(handle.clone());
    let inputs = resolve(&root, &request)?;
    let docker = SshDockerInventoryAdapter {
        ssh: &ssh,
        credentials: &OsCredentialStore,
    };
    let composition = composition(&inputs, &ssh, &docker);
    let plan = composition
        .plan(&inputs.backup_id)
        .map_err(|_| ReplacementFailure::rejected())?;
    plan.approve(confirmation)
        .map_err(|_| ReplacementFailure::confirmation())?;
    let manifest = inputs
        .repository
        .load_verified_manifest(&inputs.backup_id, &inputs.identity)
        .map_err(|_| ReplacementFailure::rejected())?;
    validate_safety_plan(&root, &request, &manifest)?;
    let safety_run =
        RunId::parse(random_id("safety")).map_err(|_| ReplacementFailure::storage())?;
    let safety = job_commands::run_blocking(
        root,
        job_commands::RunCapturePlanRequest {
            plan_id: manifest.plan.plan_id.as_str().to_owned(),
            run_id: safety_run.as_str().to_owned(),
        },
        safety_run,
        handle.clone(),
    )
    .map_err(|_| ReplacementFailure::safety_backup())?;
    let safety_backup_id =
        BackupId::parse(&safety.backup_id).map_err(|_| ReplacementFailure::storage())?;
    let expected_profile = inputs.profile_id.clone();
    let completed = composition
        .execute(
            &run_id,
            &expected_profile,
            &inputs.backup_id,
            &safety_backup_id,
            confirmation,
        )
        .map_err(|_| {
            if handle.is_cancelled() {
                ReplacementFailure::cancelled()
            } else {
                ReplacementFailure::rejected()
            }
        })?;
    let rollback = rollback_path(completed.impact.root.as_str(), &run_id);
    Ok(result(completed.impact, Some(safety.backup_id), &rollback))
}

fn validate_safety_plan(
    root: &Path,
    request: &ReplacementRequest,
    manifest: &guardian_core::Manifest,
) -> Result<(), ReplacementFailure> {
    let repository_id = RepositoryId::parse(&request.repository_id)
        .map_err(|_| ReplacementFailure::safety_backup())?;
    let valid = CapturePlanStore::at(root.join("plans"))
        .list()
        .map_err(|_| ReplacementFailure::storage())?
        .into_iter()
        .any(|stored| {
            stored.plan.plan_id == manifest.plan.plan_id
                && stored.plan.version == manifest.plan.version
                && stored.sha256 == manifest.plan.sha256
                && stored.plan.profile_id == manifest.source.profile_id
                && stored.plan.repository_id == repository_id
        });
    valid
        .then_some(())
        .ok_or_else(ReplacementFailure::safety_backup)
}

struct ReplacementInputs {
    repository: guardian_local_repository::LocalRepository,
    profile: guardian_core::VdsProfile,
    identity: guardian_signing::VerificationIdentity,
    backup_id: BackupId,
    profile_id: ProfileId,
}

fn resolve(
    root: &Path,
    request: &ReplacementRequest,
) -> Result<ReplacementInputs, ReplacementFailure> {
    let backup_id =
        BackupId::parse(&request.backup_id).map_err(|_| ReplacementFailure::rejected())?;
    let profile_id =
        ProfileId::parse(&request.target_profile_id).map_err(|_| ReplacementFailure::rejected())?;
    let (repository, identity) = restore_commands::resolve_repository(root, &request.repository_id)
        .map_err(|_| ReplacementFailure::storage())?;
    let profile = ProfileStore::at(root.join("profiles"))
        .get(&profile_id)
        .map_err(|_| ReplacementFailure::storage())?
        .ok_or_else(ReplacementFailure::rejected)?;
    Ok(ReplacementInputs {
        repository,
        profile,
        identity,
        backup_id,
        profile_id,
    })
}

fn composition<'a>(
    inputs: &'a ReplacementInputs,
    ssh: &'a SystemOpenSsh,
    docker_inventory: &'a dyn guardian_core::DockerInventoryPort,
) -> ReplacementComposition<'a> {
    ReplacementComposition {
        repository: &inputs.repository,
        ssh,
        target_profile: &inputs.profile,
        credentials: &OsCredentialStore,
        verifier: &inputs.identity,
        docker_inventory,
    }
}

fn result(
    impact: SourceReplacementImpact,
    safety_backup_id: Option<String>,
    rollback_path: &str,
) -> ReplacementResult {
    ReplacementResult {
        impact,
        safety_backup_id,
        rollback_path: rollback_path.to_owned(),
    }
}

fn rollback_path(root: &str, run_id: &RunId) -> String {
    let parent = root.rsplit_once('/').map(|value| value.0).unwrap_or("");
    format!("{parent}/.guardian-rollback.{run_id}")
}

fn parse_run_id(request: &ReplacementRequest) -> Result<RunId, ReplacementFailure> {
    request
        .run_id
        .as_deref()
        .ok_or_else(ReplacementFailure::storage)
        .and_then(|value| RunId::parse(value).map_err(|_| ReplacementFailure::storage()))
}

fn random_id(prefix: &str) -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    format!(
        "{prefix}-{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

impl ReplacementFailure {
    fn rejected() -> Self {
        Self {
            code: "replacement_rejected",
            message: "The managed replacement was rejected.",
            remediation: "Refresh the backup preview and resolve every conflict before trying again.",
        }
    }
    fn confirmation() -> Self {
        Self {
            code: "replacement_confirmation_required",
            message: "Exact replacement confirmation is required.",
            remediation: "Copy the confirmation phrase from the preview.",
        }
    }
    fn safety_backup() -> Self {
        Self {
            code: "replacement_safety_backup_failed",
            message: "The mandatory safety backup did not seal successfully.",
            remediation: "Fix capture access or storage; no source data was changed.",
        }
    }
    fn cancelled() -> Self {
        Self {
            code: "replacement_cancelled",
            message: "The replacement was cancelled.",
            remediation: "Review the source and rollback path before retrying.",
        }
    }
    fn storage() -> Self {
        Self {
            code: "local_storage_unavailable",
            message: "Local application storage is unavailable.",
            remediation: "Check the repository and application storage.",
        }
    }
}
