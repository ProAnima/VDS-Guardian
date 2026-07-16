//! Restore tools: `preview_restore` builds a plan and returns its
//! confirmation phrase; `execute_restore` requires that exact phrase back,
//! passed straight through to `RestorePlan::approve` with no change to that
//! logic — the calling agent must supply it explicitly every time, standing
//! in for the human who would otherwise type or paste it. Not cancellable:
//! local restore extraction has no SSH child and no cancellation path in
//! this codebase yet (a pre-existing gap, not closed here).

use crate::config::ServerConfig;
use crate::secret_store::resolve_store;
use guardian_configuration::RepositoryStore;
use guardian_core::{BackupId, RepositoryId};
use guardian_local_repository::LocalRepository;
use guardian_signing::{PortableVerificationKey, SigningIdentityManager, VerificationIdentity};
use serde::Serialize;

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RestoreFailure {
    pub code: &'static str,
    pub message: &'static str,
}

impl RestoreFailure {
    fn storage() -> Self {
        Self {
            code: "storage_unavailable",
            message: "The repository or signing identity could not be read.",
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
            message: "The repository or backup was not found.",
        }
    }
    fn rejected() -> Self {
        Self {
            code: "restore_rejected",
            message: "The restore could not be verified safely.",
        }
    }
    fn confirmation() -> Self {
        Self {
            code: "confirmation_required",
            message: "Exact restore confirmation is required.",
        }
    }
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RestorePreview {
    pub backup_id: String,
    pub destination: String,
    pub confirmation: String,
    pub payload: String,
}

impl From<guardian_core::RestorePlan> for RestorePreview {
    fn from(plan: guardian_core::RestorePlan) -> Self {
        Self {
            backup_id: plan.backup_id.as_str().to_owned(),
            destination: plan.destination.display().to_string(),
            confirmation: plan.confirmation,
            payload: plan.filesystem_payload.as_str().to_owned(),
        }
    }
}

pub(crate) fn preview_restore(
    config: &ServerConfig,
    repository_id: &str,
    backup_id: &str,
    destination: &str,
) -> Result<RestorePreview, RestoreFailure> {
    let (repository, backup_id, identity) = resolve(config, repository_id, backup_id)?;
    repository
        .plan_restore(&backup_id, destination, &identity)
        .map(RestorePreview::from)
        .map_err(|_| RestoreFailure::rejected())
}

pub(crate) fn execute_restore(
    config: &ServerConfig,
    repository_id: &str,
    backup_id: &str,
    destination: &str,
    confirmation: &str,
) -> Result<RestorePreview, RestoreFailure> {
    if confirmation.is_empty() {
        return Err(RestoreFailure::confirmation());
    }
    let secrets =
        resolve_store(config.vault_dir.as_deref()).map_err(|_| RestoreFailure::storage())?;
    let (repository, backup_id, identity) = resolve(config, repository_id, backup_id)?;
    repository
        .execute_restore(&backup_id, destination, confirmation, &identity, &secrets)
        .map(RestorePreview::from)
        .map_err(|_| RestoreFailure::rejected())
}

fn resolve(
    config: &ServerConfig,
    repository_id: &str,
    backup_id: &str,
) -> Result<(LocalRepository, BackupId, VerificationIdentity), RestoreFailure> {
    let repository_id =
        RepositoryId::parse(repository_id).map_err(|_| RestoreFailure::not_found())?;
    let registration = RepositoryStore::at(&config.repositories_dir)
        .get(&repository_id)
        .map_err(|_| RestoreFailure::storage())?
        .ok_or_else(RestoreFailure::not_found)?;
    let repository = LocalRepository::open(&registration.path, repository_id)
        .map_err(|_| RestoreFailure::storage())?;
    let secrets =
        resolve_store(config.vault_dir.as_deref()).map_err(|_| RestoreFailure::storage())?;
    let portable = repository
        .trusted_verification_key()
        .map_err(|_| RestoreFailure::storage())?
        .map(|key| PortableVerificationKey {
            algorithm: key.algorithm,
            key_id: key.key_id,
            public_key_base64: key.public_key_base64,
        });
    let identity = SigningIdentityManager::open(&config.config_dir)
        .map_err(|_| RestoreFailure::storage())?
        .load_verifier(&secrets, portable.as_ref())
        .map_err(|_| RestoreFailure::signing())?;
    let backup_id = BackupId::parse(backup_id).map_err(|_| RestoreFailure::not_found())?;
    Ok((repository, backup_id, identity))
}
