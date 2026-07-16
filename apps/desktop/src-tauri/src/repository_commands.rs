use guardian_configuration::{RepositoryRegistration, RepositoryStore};
use guardian_core::RepositoryId;
use guardian_local_repository::{LocalRepository, RepositoryError, RepositoryVerificationKey};
use guardian_os_keyring::OsCredentialStore;
use guardian_signing::SigningIdentityManager;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Manager;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRepositoryRequest {
    label: String,
    path: String,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RepositorySummary {
    pub repository_id: String,
    pub label: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryCommandFailure {
    pub code: &'static str,
    pub message: &'static str,
    pub remediation: &'static str,
}

pub async fn register(
    app: tauri::AppHandle,
    request: RegisterRepositoryRequest,
) -> Result<RepositorySummary, RepositoryCommandFailure> {
    let root = registry_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || register_blocking(root, request))
        .await
        .map_err(|_| RepositoryCommandFailure::internal())?
}

pub async fn list(
    app: tauri::AppHandle,
) -> Result<Vec<RepositorySummary>, RepositoryCommandFailure> {
    let root = registry_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        RepositoryStore::at(root)
            .list()
            .map(|entries| entries.iter().map(RepositorySummary::from).collect())
            .map_err(|_| RepositoryCommandFailure::storage())
    })
    .await
    .map_err(|_| RepositoryCommandFailure::internal())?
}

pub async fn initialize_recovery(
    app: tauri::AppHandle,
    repository_id: String,
) -> Result<(), RepositoryCommandFailure> {
    let root = app
        .path()
        .app_config_dir()
        .map_err(|_| RepositoryCommandFailure::storage())?;
    tauri::async_runtime::spawn_blocking(move || initialize_recovery_blocking(root, repository_id))
        .await
        .map_err(|_| RepositoryCommandFailure::internal())?
}

fn initialize_recovery_blocking(
    root: PathBuf,
    repository_id: String,
) -> Result<(), RepositoryCommandFailure> {
    let repository_id =
        RepositoryId::parse(repository_id).map_err(|_| RepositoryCommandFailure::invalid())?;
    let registration = RepositoryStore::at(root.join("repositories"))
        .get(&repository_id)
        .map_err(|_| RepositoryCommandFailure::storage())?
        .ok_or_else(RepositoryCommandFailure::invalid)?;
    let repository = LocalRepository::open(&registration.path, repository_id)
        .map_err(|_| RepositoryCommandFailure::repository())?;
    let identity = SigningIdentityManager::open(root.join("node"))
        .map_err(|_| RepositoryCommandFailure::signing())?
        .load_ready(&OsCredentialStore)
        .map_err(|_| RepositoryCommandFailure::signing())?;
    let key = identity.verification_key();
    repository
        .pin_verification_key(RepositoryVerificationKey {
            algorithm: key.algorithm,
            key_id: key.key_id,
            public_key_base64: key.public_key_base64,
        })
        .map_err(|_| RepositoryCommandFailure::recovery())?;
    match repository.configure_recovery_key(&OsCredentialStore) {
        Ok(_) | Err(RepositoryError::RecoveryKeyAlreadyConfigured) => Ok(()),
        Err(_) => Err(RepositoryCommandFailure::recovery()),
    }
}

fn register_blocking(
    root: PathBuf,
    request: RegisterRepositoryRequest,
) -> Result<RepositorySummary, RepositoryCommandFailure> {
    let id = RepositoryId::parse(random_id()).map_err(|_| RepositoryCommandFailure::internal())?;
    let repository = LocalRepository::open(&request.path, id.clone())
        .map_err(|_| RepositoryCommandFailure::repository())?;
    let registration = RepositoryRegistration::new(id, request.label, repository.root().to_owned())
        .map_err(|_| RepositoryCommandFailure::invalid())?;
    RepositoryStore::at(root)
        .upsert(registration.clone())
        .map_err(|_| RepositoryCommandFailure::storage())?;
    Ok(RepositorySummary::from(&registration))
}

fn random_id() -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    format!(
        "repository-{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn registry_root(app: &tauri::AppHandle) -> Result<PathBuf, RepositoryCommandFailure> {
    app.path()
        .app_config_dir()
        .map(|path| path.join("repositories"))
        .map_err(|_| RepositoryCommandFailure::storage())
}

impl From<&RepositoryRegistration> for RepositorySummary {
    fn from(value: &RepositoryRegistration) -> Self {
        Self {
            repository_id: value.repository_id.as_str().to_owned(),
            label: value.label.clone(),
            path: value.path.display().to_string(),
        }
    }
}

impl RepositoryCommandFailure {
    fn invalid() -> Self {
        Self {
            code: "invalid_repository",
            message: "The backup location is invalid.",
            remediation: "Use a dedicated, existing folder that is not a symbolic link.",
        }
    }
    fn repository() -> Self {
        Self {
            code: "repository_unavailable",
            message: "The backup repository could not be initialized.",
            remediation: "Choose a writable dedicated folder that is not already owned by another repository.",
        }
    }
    fn storage() -> Self {
        Self {
            code: "repository_registry_unavailable",
            message: "The backup location could not be registered.",
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
    fn signing() -> Self {
        Self {
            code: "signing_identity_unavailable",
            message: "The signing identity is not ready.",
            remediation: "Create the signing identity first, then prepare repository recovery.",
        }
    }
    fn recovery() -> Self {
        Self {
            code: "recovery_setup_failed",
            message: "Recovery protection could not be prepared.",
            remediation: "Check credential-store access and retry before starting a backup.",
        }
    }
}
