use guardian_core::{
    DiscoverDockerInventoryUseCase, DockerContainerState, DockerMountKind, ProfileId,
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
pub struct DockerContainerSummary {
    pub id: String,
    pub name: String,
    pub state: DockerContainerState,
    pub mounts: Vec<DockerMountSummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerMountSummary {
    pub kind: DockerMountKind,
    pub destination: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capturable_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerCommandFailure {
    pub code: &'static str,
    pub message: &'static str,
    pub remediation: &'static str,
}

pub async fn list_containers(
    app: tauri::AppHandle,
    profile_id: String,
) -> Result<Vec<DockerContainerSummary>, DockerCommandFailure> {
    let root = app
        .path()
        .app_config_dir()
        .map_err(|_| DockerCommandFailure::storage())?;
    tauri::async_runtime::spawn_blocking(move || list_containers_blocking(root, profile_id))
        .await
        .map_err(|_| DockerCommandFailure::internal())?
}

fn list_containers_blocking(
    root: PathBuf,
    profile_id: String,
) -> Result<Vec<DockerContainerSummary>, DockerCommandFailure> {
    let id = ProfileId::parse(profile_id).map_err(|_| DockerCommandFailure::not_found())?;
    let profiles = ProfileStore::at(root.join("profiles"));
    let ssh = SystemOpenSsh::default();
    let adapter = SshDockerInventoryAdapter {
        ssh: &ssh,
        credentials: &OsCredentialStore,
    };
    let inventory = DiscoverDockerInventoryUseCase {
        profiles: &profiles,
        inventory: &adapter,
    }
    .execute(&id)
    .map_err(|_| DockerCommandFailure::inspection_failed())?;
    Ok(inventory
        .containers
        .iter()
        .map(DockerContainerSummary::from)
        .collect())
}

impl From<&guardian_core::DockerContainer> for DockerContainerSummary {
    fn from(container: &guardian_core::DockerContainer) -> Self {
        Self {
            id: container.id.clone(),
            name: container.name.clone(),
            state: container.state,
            mounts: container
                .mounts
                .iter()
                .map(DockerMountSummary::from)
                .collect(),
        }
    }
}

impl From<&guardian_core::DockerMount> for DockerMountSummary {
    fn from(mount: &guardian_core::DockerMount) -> Self {
        Self {
            kind: mount.kind,
            destination: mount.destination.clone(),
            capturable_path: mount.capturable_path().map(ToOwned::to_owned),
        }
    }
}

impl DockerCommandFailure {
    fn not_found() -> Self {
        Self {
            code: "docker_profile_not_found",
            message: "The server profile was not found.",
            remediation: "Refresh the server list and select an enrolled server.",
        }
    }
    fn inspection_failed() -> Self {
        Self {
            code: "docker_inspection_failed",
            message: "Could not read Docker containers from this server.",
            remediation: "Confirm Docker is installed and the backup account can run `docker inspect`.",
        }
    }
    fn storage() -> Self {
        Self {
            code: "local_storage_unavailable",
            message: "Local application storage is unavailable.",
            remediation: "Check local storage and try again.",
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
