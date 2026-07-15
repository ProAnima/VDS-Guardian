use crate::RepositoryError;
use crate::filesystem::ensure_directory;
use crate::signature_file::DiskSignature;
use crate::verification::verify_sealed_payloads;
use guardian_core::{BackupId, Manifest, ManifestVerifier, Timestamp};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedBackup {
    pub backup_id: BackupId,
    pub sealed_at: Timestamp,
}

pub(crate) fn trusted_inventory(
    backups_root: &Path,
    verifier: &dyn ManifestVerifier,
) -> Result<Vec<TrustedBackup>, RepositoryError> {
    ensure_directory(backups_root)?;
    let mut inventory = Vec::new();
    for entry in fs::read_dir(backups_root)
        .map_err(|source| RepositoryError::io("list sealed backups", source))?
    {
        let entry = entry.map_err(|source| RepositoryError::io("read backup entry", source))?;
        inventory.push(inspect_backup(entry.path(), verifier)?);
    }
    inventory.sort_by(|left, right| {
        left.sealed_at
            .as_str()
            .cmp(right.sealed_at.as_str())
            .then_with(|| left.backup_id.as_str().cmp(right.backup_id.as_str()))
    });
    Ok(inventory)
}

fn inspect_backup(
    path: PathBuf,
    verifier: &dyn ManifestVerifier,
) -> Result<TrustedBackup, RepositoryError> {
    let manifest = load_verified_manifest(&path, verifier)?;
    Ok(TrustedBackup {
        backup_id: manifest.backup_id,
        sealed_at: manifest
            .sealed_at
            .ok_or(RepositoryError::IntegrityFailure)?,
    })
}

pub(crate) fn load_verified_manifest(
    path: &Path,
    verifier: &dyn ManifestVerifier,
) -> Result<Manifest, RepositoryError> {
    ensure_directory(path)?;
    let directory_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(RepositoryError::UnsafeFilesystemEntry)?;
    let directory_id =
        BackupId::parse(directory_name).map_err(|_| RepositoryError::UnsafeFilesystemEntry)?;
    let canonical = read_regular_file(&path.join("manifest.json"), "read sealed manifest")?;
    let manifest: Manifest =
        serde_json::from_slice(&canonical).map_err(|_| RepositoryError::IntegrityFailure)?;
    manifest.validate_sealed()?;
    if manifest.backup_id != directory_id || manifest.canonical_bytes()? != canonical {
        return Err(RepositoryError::IntegrityFailure);
    }
    verify_signature(path, &manifest, &canonical, verifier)?;
    verify_sealed_payloads(path, &manifest)?;
    Ok(manifest)
}

fn verify_signature(
    path: &Path,
    manifest: &Manifest,
    canonical: &[u8],
    verifier: &dyn ManifestVerifier,
) -> Result<(), RepositoryError> {
    let bytes = read_regular_file(&path.join("manifest.sig"), "read manifest signature")?;
    let disk: DiskSignature =
        serde_json::from_slice(&bytes).map_err(|_| RepositoryError::IntegrityFailure)?;
    let metadata = manifest
        .signature
        .as_ref()
        .ok_or(RepositoryError::IntegrityFailure)?;
    if disk.algorithm != metadata.algorithm || disk.key_id != metadata.key_id {
        return Err(RepositoryError::IntegrityFailure);
    }
    verifier.verify_manifest(&disk.algorithm, &disk.key_id, canonical, &disk.decode()?)?;
    Ok(())
}

fn read_regular_file(path: &Path, operation: &'static str) -> Result<Vec<u8>, RepositoryError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| RepositoryError::io("inspect sealed metadata file", source))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(RepositoryError::UnsafeFilesystemEntry);
    }
    fs::read(path).map_err(|source| RepositoryError::io(operation, source))
}
