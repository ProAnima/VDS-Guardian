//! Deploy tools: `preview_deploy` builds a plan and returns its confirmation
//! phrase; `execute_deploy` requires that exact phrase back, passed straight
//! through to `DeploymentPlan::approve` with no change to that logic.
//! Cancellable (SSH-backed), via the caller-supplied `run_id` registered in
//! the shared `JobRegistry` before the push begins.

use crate::config::ServerConfig;
use crate::secret_store::resolve_store;
use guardian_configuration::RepositoryStore;
use guardian_core::{
    BackupId, CancellationHandle, JobRegistry, ProfileId, ProfileStorePort, RemoteTargetPath,
    RepositoryId, RunId,
};
use guardian_deploy::DeploymentComposition;
use guardian_local_repository::LocalRepository;
use guardian_profile_store::ProfileStore;
use guardian_signing::{PortableVerificationKey, SigningIdentityManager, VerificationIdentity};
use guardian_ssh::SystemOpenSsh;
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeployFailure {
    pub code: &'static str,
    pub message: &'static str,
}

impl DeployFailure {
    fn storage() -> Self {
        Self {
            code: "storage_unavailable",
            message: "The repository, profile store, or signing identity could not be read.",
        }
    }
    fn signing() -> Self {
        Self {
            code: "signing_identity_unavailable",
            message: "This node has no ready signing identity to verify backups with.",
        }
    }
    fn not_found() -> Self {
        Self {
            code: "not_found",
            message: "The repository, backup, or target profile was not found.",
        }
    }
    fn invalid_target_path() -> Self {
        Self {
            code: "invalid_target_path",
            message: "The target path must be an absolute POSIX path on the remote host.",
        }
    }
    fn rejected() -> Self {
        Self {
            code: "deploy_rejected",
            message: "The deployment could not be verified or pushed safely.",
        }
    }
    fn confirmation() -> Self {
        Self {
            code: "confirmation_required",
            message: "Exact deploy confirmation is required.",
        }
    }
    fn cancelled() -> Self {
        Self {
            code: "deploy_cancelled",
            message: "The deploy was cancelled by the operator.",
        }
    }
    fn internal() -> Self {
        Self {
            code: "internal_error",
            message: "The deploy request could not be processed.",
        }
    }
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
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

struct ResolvedDeployInputs {
    repository: LocalRepository,
    target_profile: guardian_core::VdsProfile,
    identity: VerificationIdentity,
}

fn resolve(
    config: &ServerConfig,
    repository_id: &str,
    backup_id: &str,
    target_profile_id: &str,
    target_path: &str,
) -> Result<(ResolvedDeployInputs, BackupId, RemoteTargetPath), DeployFailure> {
    let repository_id =
        RepositoryId::parse(repository_id).map_err(|_| DeployFailure::not_found())?;
    let registration = RepositoryStore::at(&config.repositories_dir)
        .get(&repository_id)
        .map_err(|_| DeployFailure::storage())?
        .ok_or_else(DeployFailure::not_found)?;
    let repository = LocalRepository::open(&registration.path, repository_id)
        .map_err(|_| DeployFailure::storage())?;
    let secrets =
        resolve_store(config.vault_dir.as_deref()).map_err(|_| DeployFailure::storage())?;
    let portable = repository
        .trusted_verification_key()
        .map_err(|_| DeployFailure::storage())?
        .map(|key| PortableVerificationKey {
            algorithm: key.algorithm,
            key_id: key.key_id,
            public_key_base64: key.public_key_base64,
        });
    let identity = SigningIdentityManager::open(&config.config_dir)
        .map_err(|_| DeployFailure::storage())?
        .load_verifier(&secrets, portable.as_ref())
        .map_err(|_| DeployFailure::signing())?;
    let backup_id = BackupId::parse(backup_id).map_err(|_| DeployFailure::not_found())?;
    let target_profile_id =
        ProfileId::parse(target_profile_id).map_err(|_| DeployFailure::not_found())?;
    let target_profile = ProfileStore::at(&config.profiles_dir)
        .get(&target_profile_id)
        .map_err(|_| DeployFailure::storage())?
        .ok_or_else(DeployFailure::not_found)?;
    let target_path =
        RemoteTargetPath::parse(target_path).map_err(|_| DeployFailure::invalid_target_path())?;
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

pub(crate) fn preview_deploy(
    config: &ServerConfig,
    repository_id: &str,
    backup_id: &str,
    target_profile_id: &str,
    target_path: &str,
) -> Result<DeploymentPreview, DeployFailure> {
    let (inputs, backup_id, target_path) = resolve(
        config,
        repository_id,
        backup_id,
        target_profile_id,
        target_path,
    )?;
    let ssh = SystemOpenSsh::default();
    let secrets =
        resolve_store(config.vault_dir.as_deref()).map_err(|_| DeployFailure::storage())?;
    let composition = DeploymentComposition {
        repository: &inputs.repository,
        ssh: &ssh,
        target_profile: &inputs.target_profile,
        credentials: &secrets,
        verifier: &inputs.identity,
    };
    composition
        .plan(&backup_id, target_path)
        .map(|plan| summary(plan, &inputs.target_profile))
        .map_err(|_| DeployFailure::rejected())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_deploy(
    config: &ServerConfig,
    jobs: &Arc<JobRegistry>,
    repository_id: &str,
    backup_id: &str,
    target_profile_id: &str,
    target_path: &str,
    confirmation: &str,
    run_id: &str,
) -> Result<DeploymentPreview, DeployFailure> {
    if confirmation.is_empty() {
        return Err(DeployFailure::confirmation());
    }
    let run_id = RunId::parse(run_id).map_err(|_| DeployFailure::internal())?;
    let handle = CancellationHandle::new();
    // Registered before the push itself starts, so a concurrent `cancel_job`
    // tool call can find it while this deploy is still in flight.
    let _registration = jobs.register(run_id.clone(), handle.clone());
    let (inputs, backup_id, target_path) = resolve(
        config,
        repository_id,
        backup_id,
        target_profile_id,
        target_path,
    )?;
    let target_profile_id = inputs.target_profile.profile_id.clone();
    let ssh = SystemOpenSsh::default().with_cancellation(handle.clone());
    let secrets =
        resolve_store(config.vault_dir.as_deref()).map_err(|_| DeployFailure::storage())?;
    let composition = DeploymentComposition {
        repository: &inputs.repository,
        ssh: &ssh,
        target_profile: &inputs.target_profile,
        credentials: &secrets,
        verifier: &inputs.identity,
    };
    match composition.execute(
        &run_id,
        &target_profile_id,
        &backup_id,
        target_path,
        confirmation,
    ) {
        Ok(plan) => Ok(summary(plan, &inputs.target_profile)),
        Err(_) if handle.is_cancelled() => Err(DeployFailure::cancelled()),
        Err(_) => Err(DeployFailure::rejected()),
    }
}
