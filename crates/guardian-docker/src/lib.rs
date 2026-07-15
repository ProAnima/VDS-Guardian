//! Bounded parser for the reviewed `docker inspect` JSON response.

use guardian_core::{
    DockerContainer, DockerContainerState, DockerHealth, DockerInventory, DockerInventoryPort,
    DockerInventoryPortError, DockerMount, DockerMountKind, DockerNetwork, SecretStore, VdsProfile,
};
use guardian_ssh::{PinnedHost, SshIdentity, SshUser, SystemOpenSsh};
use serde::Deserialize;
use std::{collections::BTreeMap, fs};
use tempfile::tempdir;
use thiserror::Error;

pub const MAX_INSPECT_BYTES: usize = 8 * 1024 * 1024;

pub struct SshDockerInventoryAdapter<'a> {
    pub ssh: &'a SystemOpenSsh,
    pub credentials: &'a dyn SecretStore,
}

impl DockerInventoryPort for SshDockerInventoryAdapter<'_> {
    fn inspect(&self, profile: &VdsProfile) -> Result<DockerInventory, DockerInventoryPortError> {
        profile
            .validate()
            .map_err(|_| DockerInventoryPortError::Rejected)?;
        let host = PinnedHost::parse(
            &profile.endpoint.host,
            profile.endpoint.port,
            &profile.endpoint.host_pin.algorithm,
            &profile.endpoint.host_pin.public_key_base64,
        )
        .map_err(|_| DockerInventoryPortError::Rejected)?;
        let user = SshUser::parse(&profile.endpoint.user)
            .map_err(|_| DockerInventoryPortError::Rejected)?;
        let identity = SshIdentity::from_store(self.credentials, &profile.credential_id)
            .map_err(|_| DockerInventoryPortError::Unavailable)?;
        let temporary = tempdir().map_err(|_| DockerInventoryPortError::Unavailable)?;
        let destination = temporary.path().join("docker-inspect.json");
        self.ssh
            .inspect_docker_to(
                &host,
                &user,
                identity.path(),
                &destination,
                u64::try_from(MAX_INSPECT_BYTES)
                    .map_err(|_| DockerInventoryPortError::Unavailable)?,
            )
            .map_err(|_| DockerInventoryPortError::Unavailable)?;
        let bytes = fs::read(&destination).map_err(|_| DockerInventoryPortError::Unavailable)?;
        parse_inspect_json(if bytes.is_empty() { b"[]" } else { &bytes })
            .map_err(|_| DockerInventoryPortError::Rejected)
    }
}

pub fn parse_inspect_json(bytes: &[u8]) -> Result<DockerInventory, DockerInspectError> {
    if bytes.len() > MAX_INSPECT_BYTES {
        return Err(DockerInspectError::Rejected);
    }
    let inspected: Vec<InspectRecord> =
        serde_json::from_slice(bytes).map_err(|_| DockerInspectError::Rejected)?;
    let inventory = DockerInventory {
        containers: inspected
            .into_iter()
            .map(InspectRecord::into_container)
            .collect::<Result<_, _>>()?,
    };
    inventory
        .validate()
        .map_err(|_| DockerInspectError::Rejected)?;
    Ok(inventory)
}

#[derive(Deserialize)]
struct InspectRecord {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Image", default)]
    image_digest: String,
    #[serde(rename = "Config")]
    config: InspectConfig,
    #[serde(rename = "State")]
    state: InspectState,
    #[serde(rename = "Mounts", default)]
    mounts: Vec<InspectMount>,
    #[serde(rename = "NetworkSettings", default)]
    networks: InspectNetworks,
}

impl InspectRecord {
    fn into_container(self) -> Result<DockerContainer, DockerInspectError> {
        let name = self
            .name
            .strip_prefix('/')
            .filter(|value| !value.is_empty())
            .ok_or(DockerInspectError::Rejected)?
            .to_owned();
        let secret_references = self
            .mounts
            .iter()
            .filter_map(InspectMount::secret_reference)
            .collect();
        Ok(DockerContainer {
            id: self.id,
            name,
            image: self.config.image,
            image_digest: self
                .image_digest
                .starts_with("sha256:")
                .then_some(self.image_digest),
            compose_project: self
                .config
                .labels
                .get("com.docker.compose.project")
                .cloned(),
            state: parse_state(&self.state.status)?,
            health: self
                .state
                .health
                .map(|health| parse_health(&health.status))
                .transpose()?,
            mounts: self
                .mounts
                .into_iter()
                .map(InspectMount::into_mount)
                .collect::<Result<_, _>>()?,
            networks: self.networks.into_networks(),
            secret_references,
        })
    }
}

#[derive(Deserialize)]
struct InspectConfig {
    #[serde(rename = "Image")]
    image: String,
    #[serde(rename = "Labels", default)]
    labels: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct InspectState {
    #[serde(rename = "Status")]
    status: String,
    #[serde(rename = "Health")]
    health: Option<InspectHealth>,
}

#[derive(Deserialize)]
struct InspectHealth {
    #[serde(rename = "Status")]
    status: String,
}

#[derive(Deserialize)]
struct InspectMount {
    #[serde(rename = "Type")]
    kind: String,
    #[serde(rename = "Source", default)]
    source: String,
    #[serde(rename = "Name", default)]
    name: String,
    #[serde(rename = "Destination")]
    destination: String,
    #[serde(rename = "RW")]
    read_write: bool,
}

impl InspectMount {
    fn into_mount(self) -> Result<DockerMount, DockerInspectError> {
        let kind = parse_mount_kind(&self.kind)?;
        let host_path = match kind {
            DockerMountKind::Volume if !self.source.is_empty() => Some(self.source.clone()),
            _ => None,
        };
        let source_reference = match kind {
            DockerMountKind::Bind => self.source,
            DockerMountKind::Volume => self.name,
            DockerMountKind::Tmpfs => "tmpfs".to_owned(),
        };
        Ok(DockerMount {
            kind,
            source_reference,
            host_path,
            destination: self.destination,
            read_only: !self.read_write,
        })
    }

    fn secret_reference(&self) -> Option<String> {
        self.destination
            .strip_prefix("/run/secrets/")
            .filter(|name| !name.contains('/'))
            .map(ToOwned::to_owned)
    }
}

#[derive(Default, Deserialize)]
struct InspectNetworks {
    #[serde(rename = "Networks", default)]
    networks: BTreeMap<String, InspectNetwork>,
}

impl InspectNetworks {
    fn into_networks(self) -> Vec<DockerNetwork> {
        self.networks
            .into_iter()
            .map(|(name, network)| DockerNetwork {
                name,
                network_id: network.network_id,
            })
            .collect()
    }
}

#[derive(Deserialize)]
struct InspectNetwork {
    #[serde(rename = "NetworkID")]
    network_id: String,
}

fn parse_state(value: &str) -> Result<DockerContainerState, DockerInspectError> {
    match value {
        "created" => Ok(DockerContainerState::Created),
        "running" => Ok(DockerContainerState::Running),
        "paused" => Ok(DockerContainerState::Paused),
        "restarting" => Ok(DockerContainerState::Restarting),
        "exited" => Ok(DockerContainerState::Exited),
        "dead" => Ok(DockerContainerState::Dead),
        _ => Err(DockerInspectError::Rejected),
    }
}

fn parse_health(value: &str) -> Result<DockerHealth, DockerInspectError> {
    match value {
        "starting" => Ok(DockerHealth::Starting),
        "healthy" => Ok(DockerHealth::Healthy),
        "unhealthy" => Ok(DockerHealth::Unhealthy),
        "none" => Ok(DockerHealth::None),
        _ => Err(DockerInspectError::Rejected),
    }
}

fn parse_mount_kind(value: &str) -> Result<DockerMountKind, DockerInspectError> {
    match value {
        "bind" => Ok(DockerMountKind::Bind),
        "volume" => Ok(DockerMountKind::Volume),
        "tmpfs" => Ok(DockerMountKind::Tmpfs),
        _ => Err(DockerInspectError::Rejected),
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DockerInspectError {
    #[error("Docker inspect output was rejected")]
    Rejected,
}
