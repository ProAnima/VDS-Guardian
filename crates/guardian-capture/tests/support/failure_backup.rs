use super::drill_manifest;
use guardian_core::{
    BackupId, ManifestSigner, ManifestVerifier, PayloadPath, RunId, SecretStore, Timestamp,
    VdsProfile,
};
use guardian_local_repository::{LocalRepository, SealedBackup};
use std::{error::Error, io::Read};

pub fn create_second_payload_failure_backup<S>(
    repository: &LocalRepository,
    source_backup_id: &BackupId,
    credentials: &dyn SecretStore,
    signer: &S,
    profile: &VdsProfile,
) -> Result<SealedBackup, Box<dyn Error>>
where
    S: ManifestSigner + ManifestVerifier,
{
    let filesystem_path = PayloadPath::parse("payload/filesystem-000.tar.zst.enc")?;
    let (mut filesystem, _) = repository.open_deploy_payload_reader(
        source_backup_id,
        &filesystem_path,
        signer,
        credentials,
    )?;
    let mut filesystem_bytes = Vec::new();
    filesystem.read_to_end(&mut filesystem_bytes)?;

    let backup_id = BackupId::parse("drill-second-payload-failure")?;
    let run_id = RunId::parse("drill-second-payload-failure-run")?;
    let staging = repository.begin_staging(run_id.clone())?;
    staging.write_payload(
        "filesystem",
        filesystem_path.clone(),
        "application/zstd",
        &filesystem_bytes,
    )?;
    let filesystem_entry = staging.encrypt_and_register_payload_file(
        "filesystem",
        filesystem_path,
        "application/zstd",
        &backup_id,
        credentials,
    )?;

    let database_path = PayloadPath::parse("payload/database-000.sqlite.zst.enc")?;
    staging.write_payload(
        "database",
        database_path.clone(),
        "application/vnd.sqlite3+zstd",
        b"intentionally invalid zstd stream",
    )?;
    let database_entry = staging.encrypt_and_register_payload_file(
        "database",
        database_path,
        "application/vnd.sqlite3+zstd",
        &backup_id,
        credentials,
    )?;

    let mut manifest = drill_manifest(backup_id.as_str(), run_id, profile)?;
    manifest.add_payload(filesystem_entry)?;
    manifest.add_payload(database_entry)?;
    staging
        .seal(manifest, Timestamp::parse("2026-07-15T12:00:02Z")?, signer)
        .map_err(Into::into)
}
