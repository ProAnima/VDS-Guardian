//! Read-only discovery tools: list already-enrolled profiles, registered
//! repositories, saved capture plans, a server's Docker inventory, and a
//! repository's sealed backups. None of these mutate anything, so none
//! carry a confirmation gate.

use crate::{config::ServerConfig, secret_store::resolve_store};
use guardian_configuration::{CapturePlanStore, RepositoryStore};
use guardian_core::{
    BrowseRemoteDirectoryUseCase, DiscoverDockerInventoryUseCase, ProfileId, RemoteBrowsePage,
    RemoteBrowseRequest, RemotePath, RepositoryId,
};
use guardian_docker::SshDockerInventoryAdapter;
use guardian_local_repository::LocalRepository;
use guardian_os_keyring::OsCredentialStore;
use guardian_profile_store::ProfileStore;
use guardian_signing::{PortableVerificationKey, SigningIdentityManager};
use guardian_ssh::{SshRemoteBrowserAdapter, SystemOpenSsh};
use serde::Serialize;
use std::time::Duration;

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryFailure {
    pub code: &'static str,
    pub message: &'static str,
}

impl DiscoveryFailure {
    fn storage() -> Self {
        Self {
            code: "storage_unavailable",
            message: "Local application storage could not be read.",
        }
    }
    fn not_found() -> Self {
        Self {
            code: "not_found",
            message: "The requested profile or repository was not found.",
        }
    }
    fn signing() -> Self {
        Self {
            code: "signing_identity_unavailable",
            message: "This node has no ready signing identity to verify backups with.",
        }
    }
    fn inspection_failed() -> Self {
        Self {
            code: "docker_inspection_failed",
            message: "Could not read Docker containers from this server.",
        }
    }
    fn browse_failed() -> Self {
        Self {
            code: "remote_browser_unavailable",
            message: "The requested server directory could not be read safely.",
        }
    }
    fn rejected() -> Self {
        Self {
            code: "listing_rejected",
            message: "The repository's sealed backups could not be verified safely.",
        }
    }
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SshProfileSummary {
    pub profile_id: String,
    pub label: String,
    pub host: String,
    pub port: u16,
    pub user: String,
}

impl From<&guardian_core::VdsProfile> for SshProfileSummary {
    fn from(profile: &guardian_core::VdsProfile) -> Self {
        Self {
            profile_id: profile.profile_id.as_str().to_owned(),
            label: profile.label.clone(),
            host: profile.endpoint.host.clone(),
            port: profile.endpoint.port,
            user: profile.endpoint.user.clone(),
        }
    }
}

pub(crate) fn list_ssh_profiles(
    config: &ServerConfig,
) -> Result<Vec<SshProfileSummary>, DiscoveryFailure> {
    ProfileStore::at(&config.profiles_dir)
        .list()
        .map(|profiles| profiles.iter().map(SshProfileSummary::from).collect())
        .map_err(|_| DiscoveryFailure::storage())
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RepositorySummary {
    pub repository_id: String,
    pub label: String,
    pub path: String,
}

impl From<&guardian_configuration::RepositoryRegistration> for RepositorySummary {
    fn from(registration: &guardian_configuration::RepositoryRegistration) -> Self {
        Self {
            repository_id: registration.repository_id.as_str().to_owned(),
            label: registration.label.clone(),
            path: registration.path.display().to_string(),
        }
    }
}

pub(crate) fn list_repositories(
    config: &ServerConfig,
) -> Result<Vec<RepositorySummary>, DiscoveryFailure> {
    RepositoryStore::at(&config.repositories_dir)
        .list()
        .map(|registrations| registrations.iter().map(RepositorySummary::from).collect())
        .map_err(|_| DiscoveryFailure::storage())
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CapturePlanSummary {
    pub plan_id: String,
    pub profile_id: String,
    pub repository_id: String,
    pub roots: Vec<String>,
    pub database_path: Option<String>,
}

impl From<&guardian_configuration::StoredCapturePlan> for CapturePlanSummary {
    fn from(stored: &guardian_configuration::StoredCapturePlan) -> Self {
        Self {
            plan_id: stored.plan.plan_id.as_str().to_owned(),
            profile_id: stored.plan.profile_id.as_str().to_owned(),
            repository_id: stored.plan.repository_id.as_str().to_owned(),
            roots: stored.plan.roots.clone(),
            database_path: stored.plan.database_path.clone(),
        }
    }
}

pub(crate) fn list_capture_plans(
    config: &ServerConfig,
) -> Result<Vec<CapturePlanSummary>, DiscoveryFailure> {
    CapturePlanStore::at(&config.plans_dir)
        .list()
        .map(|plans| plans.iter().map(CapturePlanSummary::from).collect())
        .map_err(|_| DiscoveryFailure::storage())
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DockerContainerSummary {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compose_project: Option<String>,
    pub state: guardian_core::DockerContainerState,
    pub mounts: Vec<DockerMountSummary>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DockerMountSummary {
    pub kind: guardian_core::DockerMountKind,
    pub destination: String,
    pub capturable_path: Option<String>,
}

impl From<&guardian_core::DockerContainer> for DockerContainerSummary {
    fn from(container: &guardian_core::DockerContainer) -> Self {
        Self {
            id: container.id.clone(),
            name: container.name.clone(),
            compose_project: container.compose_project.clone(),
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

pub(crate) fn list_docker_containers(
    config: &ServerConfig,
    profile_id: &str,
) -> Result<Vec<DockerContainerSummary>, DiscoveryFailure> {
    let profile_id = ProfileId::parse(profile_id).map_err(|_| DiscoveryFailure::not_found())?;
    let profiles = ProfileStore::at(&config.profiles_dir);
    let secrets = resolve_store(config.vault_dir.as_deref())
        .map_err(|_| DiscoveryFailure::inspection_failed())?;
    let ssh = SystemOpenSsh::default();
    let adapter = SshDockerInventoryAdapter {
        ssh: &ssh,
        credentials: &secrets,
    };
    let inventory = DiscoverDockerInventoryUseCase {
        profiles: &profiles,
        inventory: &adapter,
    }
    .execute(&profile_id)
    .map_err(|_| DiscoveryFailure::inspection_failed())?;
    Ok(inventory
        .containers
        .iter()
        .map(DockerContainerSummary::from)
        .collect())
}

pub(crate) fn browse_remote_directory(
    config: &ServerConfig,
    profile_id: &str,
    directory: &str,
    cursor: Option<String>,
    limit: u16,
) -> Result<RemoteBrowsePage, DiscoveryFailure> {
    let profile_id = ProfileId::parse(profile_id).map_err(|_| DiscoveryFailure::not_found())?;
    let request = RemoteBrowseRequest {
        directory: RemotePath::parse(directory).map_err(|_| DiscoveryFailure::browse_failed())?,
        cursor,
        limit,
    };
    let profiles = ProfileStore::at(&config.profiles_dir);
    let secrets = resolve_store(config.vault_dir.as_deref())
        .map_err(|_| DiscoveryFailure::browse_failed())?;
    let ssh = SystemOpenSsh::default()
        .with_total_timeout(Duration::from_secs(30))
        .with_idle_timeout(Duration::from_secs(15));
    BrowseRemoteDirectoryUseCase {
        profiles: &profiles,
        browser: &SshRemoteBrowserAdapter {
            ssh: &ssh,
            credentials: &secrets,
        },
    }
    .execute(&profile_id, &request)
    .map_err(|_| DiscoveryFailure::browse_failed())
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupSummary {
    pub backup_id: String,
    pub sealed_at: String,
}

impl From<&guardian_local_repository::TrustedBackup> for BackupSummary {
    fn from(value: &guardian_local_repository::TrustedBackup) -> Self {
        Self {
            backup_id: value.backup_id.as_str().to_owned(),
            sealed_at: value.sealed_at.as_str().to_owned(),
        }
    }
}

pub(crate) fn list_backups(
    config: &ServerConfig,
    repository_id: &str,
) -> Result<Vec<BackupSummary>, DiscoveryFailure> {
    let repository_id =
        RepositoryId::parse(repository_id).map_err(|_| DiscoveryFailure::not_found())?;
    let registration = RepositoryStore::at(&config.repositories_dir)
        .get(&repository_id)
        .map_err(|_| DiscoveryFailure::storage())?
        .ok_or_else(DiscoveryFailure::not_found)?;
    let repository = LocalRepository::open(&registration.path, repository_id)
        .map_err(|_| DiscoveryFailure::storage())?;
    let portable = repository
        .trusted_verification_key()
        .map_err(|_| DiscoveryFailure::storage())?
        .map(|key| PortableVerificationKey {
            algorithm: key.algorithm,
            key_id: key.key_id,
            public_key_base64: key.public_key_base64,
        });
    let identity = SigningIdentityManager::open(&config.config_dir)
        .map_err(|_| DiscoveryFailure::storage())?
        .load_verifier(&OsCredentialStore, portable.as_ref())
        .map_err(|_| DiscoveryFailure::signing())?;
    repository
        .list_sealed_backups(&identity)
        .map(|inventory| inventory.iter().map(BackupSummary::from).collect())
        .map_err(|_| DiscoveryFailure::rejected())
}
