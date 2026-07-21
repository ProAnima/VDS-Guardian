use crate::{
    DockerContainerState, DockerInventory, ProfileId, RemotePath, RepositoryId, SourceLayout,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use thiserror::Error;

const MAX_SELECTION_ITEMS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BackupSelection {
    pub profile_id: ProfileId,
    pub repository_id: RepositoryId,
    pub items: Vec<BackupSelectionItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sqlite_path: Option<RemotePath>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum BackupSelectionItem {
    RemotePath {
        absolute_path: RemotePath,
    },
    DockerMount {
        container_id: String,
        mount_destination: RemotePath,
        capturable_path: RemotePath,
    },
    DockerGroup {
        group_id: String,
        capturable_paths: Vec<RemotePath>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CaptureSelectionPreview {
    pub profile_id: ProfileId,
    pub repository_id: RepositoryId,
    pub normalized_roots: Vec<RemotePath>,
    pub logical_items: Vec<BackupSelectionItem>,
    pub warnings: Vec<CaptureSelectionWarning>,
    pub sqlite_path: Option<RemotePath>,
    pub confirmation: String,
    pub source_layout: SourceLayout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum CaptureSelectionWarning {
    CoveredPath {
        path: RemotePath,
        covered_by: RemotePath,
    },
    LiveDockerData {
        container_id: String,
        container_name: String,
    },
    SqliteAlsoInFilesystem {
        sqlite_path: RemotePath,
        covered_by: RemotePath,
    },
}

pub fn preview_capture_selection(
    selection: &BackupSelection,
    inventory: Option<&DockerInventory>,
) -> Result<CaptureSelectionPreview, CaptureSelectionError> {
    validate_selection(selection)?;
    if let Some(inventory) = inventory {
        inventory
            .validate()
            .map_err(|_| CaptureSelectionError::DockerSelectionChanged)?;
    }
    let mut warnings = Vec::new();
    let roots = resolve_roots(selection, inventory, &mut warnings)?;
    let normalized_roots = normalize_roots(roots, &mut warnings);
    append_sqlite_warning(
        selection.sqlite_path.as_ref(),
        &normalized_roots,
        &mut warnings,
    );
    let confirmation = confirmation(selection, &normalized_roots)?;
    let selected_container_ids = selected_container_ids(selection, inventory);
    let source_layout =
        SourceLayout::from_inventory(normalized_roots.clone(), &selected_container_ids, inventory)
            .map_err(|_| CaptureSelectionError::DockerSelectionChanged)?;
    Ok(CaptureSelectionPreview {
        profile_id: selection.profile_id.clone(),
        repository_id: selection.repository_id.clone(),
        normalized_roots,
        logical_items: selection.items.clone(),
        warnings,
        sqlite_path: selection.sqlite_path.clone(),
        confirmation,
        source_layout,
    })
}

fn selected_container_ids(
    selection: &BackupSelection,
    inventory: Option<&DockerInventory>,
) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for item in &selection.items {
        match item {
            BackupSelectionItem::DockerMount { container_id, .. } => {
                ids.insert(container_id.clone());
            }
            BackupSelectionItem::DockerGroup { group_id, .. } => {
                for container in inventory
                    .into_iter()
                    .flat_map(|value| &value.containers)
                    .filter(|container| container.compose_project.as_deref() == Some(group_id))
                {
                    ids.insert(container.id.clone());
                }
            }
            BackupSelectionItem::RemotePath { .. } => {}
        }
    }
    ids
}

fn validate_selection(selection: &BackupSelection) -> Result<(), CaptureSelectionError> {
    if selection.items.is_empty() || selection.items.len() > MAX_SELECTION_ITEMS {
        return Err(CaptureSelectionError::InvalidSelection);
    }
    for item in &selection.items {
        match item {
            BackupSelectionItem::DockerGroup {
                group_id,
                capturable_paths,
            } => {
                if !valid_group_id(group_id)
                    || capturable_paths.is_empty()
                    || capturable_paths.len() > 128
                {
                    return Err(CaptureSelectionError::InvalidSelection);
                }
            }
            BackupSelectionItem::RemotePath { .. } | BackupSelectionItem::DockerMount { .. } => {}
        }
    }
    Ok(())
}

fn resolve_roots(
    selection: &BackupSelection,
    inventory: Option<&DockerInventory>,
    warnings: &mut Vec<CaptureSelectionWarning>,
) -> Result<Vec<RemotePath>, CaptureSelectionError> {
    let mut roots = Vec::new();
    for item in &selection.items {
        match item {
            BackupSelectionItem::RemotePath { absolute_path } => roots.push(absolute_path.clone()),
            BackupSelectionItem::DockerMount {
                container_id,
                mount_destination,
                capturable_path,
            } => {
                resolve_mount(
                    inventory,
                    container_id,
                    mount_destination,
                    capturable_path,
                    warnings,
                )?;
                roots.push(capturable_path.clone());
            }
            BackupSelectionItem::DockerGroup {
                group_id,
                capturable_paths,
            } => {
                resolve_group(inventory, group_id, capturable_paths, warnings)?;
                roots.extend(capturable_paths.iter().cloned());
            }
        }
    }
    Ok(roots)
}

fn resolve_mount(
    inventory: Option<&DockerInventory>,
    container_id: &str,
    destination: &RemotePath,
    path: &RemotePath,
    warnings: &mut Vec<CaptureSelectionWarning>,
) -> Result<(), CaptureSelectionError> {
    let container = inventory
        .ok_or(CaptureSelectionError::DockerInventoryRequired)?
        .containers
        .iter()
        .find(|candidate| candidate.id == container_id)
        .ok_or(CaptureSelectionError::DockerSelectionChanged)?;
    let matches = container.mounts.iter().any(|mount| {
        mount.destination == destination.as_str() && mount.capturable_path() == Some(path.as_str())
    });
    if !matches {
        return Err(CaptureSelectionError::DockerSelectionChanged);
    }
    append_live_warning(container, warnings);
    Ok(())
}

fn resolve_group(
    inventory: Option<&DockerInventory>,
    group_id: &str,
    requested: &[RemotePath],
    warnings: &mut Vec<CaptureSelectionWarning>,
) -> Result<(), CaptureSelectionError> {
    let containers: Vec<_> = inventory
        .ok_or(CaptureSelectionError::DockerInventoryRequired)?
        .containers
        .iter()
        .filter(|container| container.compose_project.as_deref() == Some(group_id))
        .collect();
    let actual: BTreeSet<_> = containers
        .iter()
        .flat_map(|container| {
            container
                .mounts
                .iter()
                .filter_map(|mount| mount.capturable_path())
        })
        .collect();
    let requested: BTreeSet<_> = requested.iter().map(RemotePath::as_str).collect();
    if containers.is_empty() || actual != requested {
        return Err(CaptureSelectionError::DockerSelectionChanged);
    }
    for container in containers {
        append_live_warning(container, warnings);
    }
    Ok(())
}

fn append_live_warning(
    container: &crate::DockerContainer,
    warnings: &mut Vec<CaptureSelectionWarning>,
) {
    if matches!(container.state, DockerContainerState::Running | DockerContainerState::Paused | DockerContainerState::Restarting)
        && !warnings.iter().any(|warning| matches!(warning, CaptureSelectionWarning::LiveDockerData { container_id, .. } if container_id == &container.id))
    {
        warnings.push(CaptureSelectionWarning::LiveDockerData {
            container_id: container.id.clone(),
            container_name: container.name.clone(),
        });
    }
}

fn normalize_roots(
    mut roots: Vec<RemotePath>,
    warnings: &mut Vec<CaptureSelectionWarning>,
) -> Vec<RemotePath> {
    roots.sort_by(|left, right| {
        left.as_str()
            .len()
            .cmp(&right.as_str().len())
            .then_with(|| left.as_str().cmp(right.as_str()))
    });
    let mut normalized = Vec::new();
    for path in roots {
        if let Some(parent) = normalized.iter().find(|parent| covers(parent, &path)) {
            warnings.push(CaptureSelectionWarning::CoveredPath {
                path,
                covered_by: parent.clone(),
            });
        } else {
            normalized.push(path);
        }
    }
    normalized
}

fn append_sqlite_warning(
    sqlite: Option<&RemotePath>,
    roots: &[RemotePath],
    warnings: &mut Vec<CaptureSelectionWarning>,
) {
    let Some(sqlite) = sqlite else { return };
    if let Some(parent) = roots.iter().find(|root| covers(root, sqlite)) {
        warnings.push(CaptureSelectionWarning::SqliteAlsoInFilesystem {
            sqlite_path: sqlite.clone(),
            covered_by: parent.clone(),
        });
    }
}

fn covers(parent: &RemotePath, child: &RemotePath) -> bool {
    parent == child
        || parent.as_str() == "/"
        || child
            .as_str()
            .strip_prefix(parent.as_str())
            .is_some_and(|tail| tail.starts_with('/'))
}

fn confirmation(
    selection: &BackupSelection,
    roots: &[RemotePath],
) -> Result<String, CaptureSelectionError> {
    let bytes = serde_json::to_vec(&(
        selection.profile_id.as_str(),
        selection.repository_id.as_str(),
        &selection.items,
        roots,
        &selection.sqlite_path,
    ))
    .map_err(|_| CaptureSelectionError::Serialization)?;
    let digest = Sha256::digest(bytes)
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!(
        "CREATE BACKUP FOR {} IN {} {digest}",
        selection.profile_id.as_str(),
        selection.repository_id.as_str()
    ))
}

fn valid_group_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum CaptureSelectionError {
    #[error("capture selection is invalid")]
    InvalidSelection,
    #[error("Docker inventory is required for this selection")]
    DockerInventoryRequired,
    #[error("Docker selection no longer matches the server")]
    DockerSelectionChanged,
    #[error("capture selection could not be serialized")]
    Serialization,
}
