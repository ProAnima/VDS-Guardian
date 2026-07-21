use super::{RestoreFailure, resolve_repository};
use guardian_archive::{ArchiveEntryKind, ArchiveLimits, list_tar_zstd_entries};
use guardian_core::{BackupId, DockerWorkloadSnapshot, PayloadPath};
use guardian_os_keyring::OsCredentialStore;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Manager;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectBackupRequest {
    repository_id: String,
    backup_id: String,
    #[serde(default)]
    offset: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRestoreDescription {
    pub backup_id: String,
    pub source_profile_id: String,
    pub roots: Vec<String>,
    pub docker_workloads: Vec<DockerWorkloadSnapshot>,
    pub entries: Vec<BackupArchiveEntry>,
    pub total_entries: u64,
    pub next_offset: Option<u64>,
    pub replacement_available: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupArchiveEntry {
    pub path: String,
    pub kind: &'static str,
    pub size: u64,
}

pub async fn inspect_backup(
    app: tauri::AppHandle,
    request: InspectBackupRequest,
) -> Result<BackupRestoreDescription, RestoreFailure> {
    let root = app
        .path()
        .app_config_dir()
        .map_err(|_| RestoreFailure::storage())?;
    tauri::async_runtime::spawn_blocking(move || inspect_blocking(root, request))
        .await
        .map_err(|_| RestoreFailure::storage())?
}

fn inspect_blocking(
    root: PathBuf,
    request: InspectBackupRequest,
) -> Result<BackupRestoreDescription, RestoreFailure> {
    let backup_id = BackupId::parse(request.backup_id).map_err(|_| RestoreFailure::rejected())?;
    let (repository, identity) = resolve_repository(&root, &request.repository_id)?;
    let manifest = repository
        .load_verified_manifest(&backup_id, &identity)
        .map_err(|_| RestoreFailure::rejected())?;
    let payload = filesystem_payload(&manifest)?;
    let (reader, _) = repository
        .open_deploy_payload_reader(&backup_id, &payload, &identity, &OsCredentialStore)
        .map_err(|_| RestoreFailure::rejected())?;
    let page = list_tar_zstd_entries(reader, ArchiveLimits::conservative(), request.offset, 200)
        .map_err(|_| RestoreFailure::rejected())?;
    let layout = manifest.source_layout;
    Ok(BackupRestoreDescription {
        backup_id: backup_id.as_str().to_owned(),
        source_profile_id: manifest.source.profile_id.as_str().to_owned(),
        roots: layout
            .as_ref()
            .map(|value| {
                value
                    .roots
                    .iter()
                    .map(|path| path.as_str().to_owned())
                    .collect()
            })
            .unwrap_or_default(),
        docker_workloads: layout
            .as_ref()
            .map(|value| value.docker_workloads.clone())
            .unwrap_or_default(),
        entries: page
            .entries
            .into_iter()
            .map(|entry| BackupArchiveEntry {
                path: entry.path,
                kind: match entry.kind {
                    ArchiveEntryKind::Directory => "directory",
                    ArchiveEntryKind::RegularFile => "regular_file",
                },
                size: entry.size,
            })
            .collect(),
        total_entries: page.total_entries,
        next_offset: page.next_offset,
        replacement_available: layout.is_some(),
    })
}

fn filesystem_payload(manifest: &guardian_core::Manifest) -> Result<PayloadPath, RestoreFailure> {
    let mut payloads = manifest
        .payloads
        .iter()
        .filter(|payload| payload.media_type == "application/zstd")
        .map(|payload| payload.path.clone());
    let payload = payloads.next().ok_or_else(RestoreFailure::rejected)?;
    if payloads.next().is_some() {
        return Err(RestoreFailure::rejected());
    }
    Ok(payload)
}
