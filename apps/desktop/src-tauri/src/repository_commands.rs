use guardian_configuration::{RepositoryRegistration, RepositoryStore};
use guardian_core::RepositoryId;
use guardian_local_repository::LocalRepository;
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
}
