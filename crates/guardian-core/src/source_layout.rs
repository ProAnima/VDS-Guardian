use crate::{DockerContainerState, DockerInventory, RemotePath};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceLayout {
    pub roots: Vec<RemotePath>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub docker_workloads: Vec<DockerWorkloadSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DockerWorkloadSnapshot {
    pub container_id: String,
    pub container_name: String,
    pub image: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compose_project: Option<String>,
    pub state: DockerContainerState,
    pub mounts: Vec<DockerMountSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DockerMountSnapshot {
    pub source_path: RemotePath,
    pub destination: RemotePath,
    pub read_only: bool,
}

impl SourceLayout {
    pub fn validate(&self) -> Result<(), SourceLayoutError> {
        if self.roots.is_empty() || self.roots.len() > 32 || self.docker_workloads.len() > 128 {
            return Err(SourceLayoutError::Invalid);
        }
        let roots = self
            .roots
            .iter()
            .map(RemotePath::as_str)
            .collect::<BTreeSet<_>>();
        if roots.len() != self.roots.len() {
            return Err(SourceLayoutError::Invalid);
        }
        let mut containers = BTreeSet::new();
        for workload in &self.docker_workloads {
            if !containers.insert(workload.container_id.as_str())
                || workload.mounts.is_empty()
                || workload.mounts.len() > 128
                || !valid_id(&workload.container_id)
                || !valid_text(&workload.container_name, 255)
                || !valid_text(&workload.image, 512)
                || workload
                    .image_digest
                    .as_deref()
                    .is_some_and(|value| !valid_digest(value))
                || workload
                    .compose_project
                    .as_deref()
                    .is_some_and(|value| !valid_label(value))
            {
                return Err(SourceLayoutError::Invalid);
            }
            let destinations = workload
                .mounts
                .iter()
                .map(|mount| mount.destination.as_str())
                .collect::<BTreeSet<_>>();
            if destinations.len() != workload.mounts.len() {
                return Err(SourceLayoutError::Invalid);
            }
            if workload.mounts.iter().any(|mount| {
                !self
                    .roots
                    .iter()
                    .any(|root| covers(root, &mount.source_path))
            }) {
                return Err(SourceLayoutError::Invalid);
            }
        }
        Ok(())
    }

    pub fn from_inventory(
        roots: Vec<RemotePath>,
        selected_container_ids: &BTreeSet<String>,
        inventory: Option<&DockerInventory>,
    ) -> Result<Self, SourceLayoutError> {
        let docker_workloads = inventory
            .into_iter()
            .flat_map(|value| &value.containers)
            .filter(|container| selected_container_ids.contains(&container.id))
            .map(|container| snapshot(container, &roots))
            .collect::<Result<Vec<_>, _>>()?;
        let layout = Self {
            roots,
            docker_workloads,
        };
        layout.validate()?;
        Ok(layout)
    }
}

fn valid_id(value: &str) -> bool {
    (12..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_text(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
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

fn snapshot(
    container: &crate::DockerContainer,
    roots: &[RemotePath],
) -> Result<DockerWorkloadSnapshot, SourceLayoutError> {
    let mounts = container
        .mounts
        .iter()
        .filter_map(|mount| {
            let source = mount.capturable_path()?;
            let source_path = RemotePath::parse(source).ok()?;
            if !roots.iter().any(|root| covers(root, &source_path)) {
                return None;
            }
            Some(DockerMountSnapshot {
                source_path,
                destination: RemotePath::parse(&mount.destination).ok()?,
                read_only: mount.read_only,
            })
        })
        .collect::<Vec<_>>();
    if mounts.is_empty() {
        return Err(SourceLayoutError::Invalid);
    }
    Ok(DockerWorkloadSnapshot {
        container_id: container.id.clone(),
        container_name: container.name.clone(),
        image: container.image.clone(),
        image_digest: container.image_digest.clone(),
        compose_project: container.compose_project.clone(),
        state: container.state,
        mounts,
    })
}

fn covers(parent: &RemotePath, child: &RemotePath) -> bool {
    parent == child
        || parent.as_str() == "/"
        || child
            .as_str()
            .strip_prefix(parent.as_str())
            .is_some_and(|tail| tail.starts_with('/'))
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum SourceLayoutError {
    #[error("backup source layout is invalid")]
    Invalid,
}
