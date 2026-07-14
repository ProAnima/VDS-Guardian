use crate::{ProfileStorePort, ProfileStorePortError, VdsProfile};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

pub trait DockerInventoryPort: Send + Sync {
    fn inspect(&self, profile: &VdsProfile) -> Result<DockerInventory, DockerInventoryPortError>;
}

pub struct DiscoverDockerInventoryUseCase<'a> {
    pub profiles: &'a dyn ProfileStorePort,
    pub inventory: &'a dyn DockerInventoryPort,
}

impl DiscoverDockerInventoryUseCase<'_> {
    pub fn execute(
        &self,
        profile: &crate::ProfileId,
    ) -> Result<DockerInventory, DiscoverDockerInventoryError> {
        let profile = self
            .profiles
            .get(profile)
            .map_err(DiscoverDockerInventoryError::ProfileStore)?
            .ok_or(DiscoverDockerInventoryError::ProfileNotFound)?;
        let inventory = self
            .inventory
            .inspect(&profile)
            .map_err(DiscoverDockerInventoryError::Inventory)?;
        inventory
            .validate()
            .map_err(DiscoverDockerInventoryError::InvalidInventory)?;
        Ok(inventory)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DockerInventory {
    pub containers: Vec<DockerContainer>,
}

impl DockerInventory {
    pub fn validate(&self) -> Result<(), DockerInventoryError> {
        if self.containers.len() > 500 {
            return Err(DockerInventoryError::TooManyContainers);
        }
        let mut ids = BTreeSet::new();
        for container in &self.containers {
            container.validate()?;
            if !ids.insert(&container.id) {
                return Err(DockerInventoryError::DuplicateContainer);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DockerContainer {
    pub id: String,
    pub name: String,
    pub image: String,
    pub image_digest: Option<String>,
    pub compose_project: Option<String>,
    pub state: DockerContainerState,
    pub health: Option<DockerHealth>,
    pub mounts: Vec<DockerMount>,
    pub networks: Vec<DockerNetwork>,
    pub secret_references: Vec<String>,
}

impl DockerContainer {
    fn validate(&self) -> Result<(), DockerInventoryError> {
        if !valid_id(&self.id) || !valid_text(&self.name, 255) || !valid_text(&self.image, 512) {
            return Err(DockerInventoryError::InvalidContainer);
        }
        if self
            .image_digest
            .as_deref()
            .is_some_and(|value| !valid_digest(value))
            || self
                .compose_project
                .as_deref()
                .is_some_and(|value| !valid_label(value))
            || self.mounts.len() > 128
            || self.networks.len() > 64
            || self.secret_references.len() > 128
        {
            return Err(DockerInventoryError::InvalidContainer);
        }
        unique(self.mounts.iter().map(|mount| &mount.destination))?;
        unique(self.networks.iter().map(|network| &network.name))?;
        unique(self.secret_references.iter())?;
        self.mounts.iter().try_for_each(DockerMount::validate)?;
        self.networks.iter().try_for_each(DockerNetwork::validate)?;
        self.secret_references
            .iter()
            .all(|secret| valid_label(secret))
            .then_some(())
            .ok_or(DockerInventoryError::InvalidSecretReference)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DockerContainerState {
    Created,
    Running,
    Paused,
    Restarting,
    Exited,
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DockerHealth {
    Starting,
    Healthy,
    Unhealthy,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DockerMount {
    pub kind: DockerMountKind,
    pub source_reference: String,
    pub destination: String,
    pub read_only: bool,
}

impl DockerMount {
    fn validate(&self) -> Result<(), DockerInventoryError> {
        (valid_text(&self.source_reference, 1_024) && valid_absolute_path(&self.destination))
            .then_some(())
            .ok_or(DockerInventoryError::InvalidMount)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DockerMountKind {
    Bind,
    Volume,
    Tmpfs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DockerNetwork {
    pub name: String,
    pub network_id: String,
}

impl DockerNetwork {
    fn validate(&self) -> Result<(), DockerInventoryError> {
        (valid_label(&self.name) && valid_id(&self.network_id))
            .then_some(())
            .ok_or(DockerInventoryError::InvalidNetwork)
    }
}

fn unique<'a>(values: impl Iterator<Item = &'a String>) -> Result<(), DockerInventoryError> {
    let mut unique_values = BTreeSet::new();
    for value in values {
        if !unique_values.insert(value) {
            return Err(DockerInventoryError::DuplicateField);
        }
    }
    Ok(())
}

fn valid_id(value: &str) -> bool {
    (12..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn valid_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_text(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}

fn valid_absolute_path(value: &str) -> bool {
    value.starts_with('/')
        && value.len() <= 1_024
        && !value.contains(['\0', '\n', '\r', '\\'])
        && value
            .split('/')
            .skip(1)
            .all(|part| !matches!(part, "" | "." | ".."))
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DockerInventoryPortError {
    #[error("Docker inventory adapter is unavailable")]
    Unavailable,
    #[error("Docker inventory adapter rejected untrusted output")]
    Rejected,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DockerInventoryError {
    #[error("Docker inventory has too many containers")]
    TooManyContainers,
    #[error("Docker inventory repeats a container")]
    DuplicateContainer,
    #[error("Docker container metadata is invalid")]
    InvalidContainer,
    #[error("Docker mount metadata is invalid")]
    InvalidMount,
    #[error("Docker network metadata is invalid")]
    InvalidNetwork,
    #[error("Docker secret reference metadata is invalid")]
    InvalidSecretReference,
    #[error("Docker inventory repeats a field")]
    DuplicateField,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DiscoverDockerInventoryError {
    #[error("profile storage failed")]
    ProfileStore(#[source] ProfileStorePortError),
    #[error("profile was not found")]
    ProfileNotFound,
    #[error("Docker inventory failed")]
    Inventory(#[source] DockerInventoryPortError),
    #[error("Docker inventory was rejected")]
    InvalidInventory(#[source] DockerInventoryError),
}
