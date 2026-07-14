use crate::RepositoryError;
use crate::filesystem::{atomic_write, create_safe_parent, sync_parent, write_new};
use crate::repository::{LocalRepository, RepositoryLock};
use crate::signature_file::DiskSignature;
use crate::verification::verify_staged_payloads;
use guardian_core::{
    BackupId, Manifest, ManifestSigner, PayloadEntry, PayloadPath, RunId, Timestamp,
    VerificationState,
};
use serde::Serialize;
use std::fs;
use std::path::PathBuf;

const SIGNATURE_ALGORITHM: &str = "Ed25519";

pub struct StagingBackup<'repository> {
    pub(crate) repository: &'repository LocalRepository,
    pub(crate) run_id: RunId,
    pub(crate) path: PathBuf,
    pub(crate) _lock: RepositoryLock,
}

impl StagingBackup<'_> {
    pub fn write_payload(
        &self,
        logical_role: impl Into<String>,
        relative_path: PayloadPath,
        media_type: impl Into<String>,
        bytes: &[u8],
    ) -> Result<PayloadEntry, RepositoryError> {
        let target = self.reserve_payload_destination(&relative_path)?;
        write_new(&target, bytes)?;
        self.register_payload_file(logical_role, relative_path, media_type)
    }

    pub fn reserve_payload_destination(
        &self,
        relative_path: &PayloadPath,
    ) -> Result<PathBuf, RepositoryError> {
        if !relative_path.as_str().starts_with("payload/") {
            return Err(RepositoryError::UnsafeFilesystemEntry);
        }
        let target = create_safe_parent(&self.path, relative_path.as_str())?;
        (!target.exists())
            .then_some(target)
            .ok_or(RepositoryError::PayloadExists)
    }

    pub fn register_payload_file(
        &self,
        logical_role: impl Into<String>,
        relative_path: PayloadPath,
        media_type: impl Into<String>,
    ) -> Result<PayloadEntry, RepositoryError> {
        let target = self.path.join(relative_path.as_str());
        let metadata = fs::symlink_metadata(&target)
            .map_err(|source| RepositoryError::io("inspect staged payload", source))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(RepositoryError::UnsafeFilesystemEntry);
        }
        PayloadEntry::new(
            logical_role,
            relative_path,
            metadata.len(),
            crate::verification::hash_file(&target)?,
            media_type,
        )
        .map_err(RepositoryError::from)
    }

    pub fn seal(
        self,
        mut manifest: Manifest,
        sealed_at: Timestamp,
        signer: &dyn ManifestSigner,
    ) -> Result<SealedBackup, RepositoryError> {
        let result = self.seal_inner(&mut manifest, sealed_at, signer);
        match result {
            Ok(sealed) => Ok(sealed),
            Err(error) => {
                self.repository.quarantine(&self.run_id, "seal_failed")?;
                Err(error)
            }
        }
    }

    fn seal_inner(
        &self,
        manifest: &mut Manifest,
        sealed_at: Timestamp,
        signer: &dyn ManifestSigner,
    ) -> Result<SealedBackup, RepositoryError> {
        if manifest.run_id != self.run_id {
            return Err(RepositoryError::RunMismatch);
        }
        if signer.algorithm() != SIGNATURE_ALGORITHM {
            return Err(RepositoryError::UnsupportedSigner);
        }
        verify_staged_payloads(&self.path, manifest)?;
        manifest.prepare_for_seal(sealed_at, signer.algorithm(), signer.key_id())?;
        let canonical = manifest.canonical_bytes()?;
        let signature = signer.sign(&canonical)?;
        signer.verify(&canonical, &signature)?;
        self.publish_metadata(manifest, &canonical, &signature)?;
        self.publish_backup(&manifest.backup_id)
    }

    fn publish_metadata(
        &self,
        manifest: &Manifest,
        canonical: &[u8],
        signature: &[u8],
    ) -> Result<(), RepositoryError> {
        let key_id = &manifest
            .signature
            .as_ref()
            .ok_or(RepositoryError::IntegrityFailure)?
            .key_id;
        let envelope = DiskSignature::new(SIGNATURE_ALGORITHM, key_id, signature);
        let report = VerificationReport {
            state: VerificationState::Verified,
            payload_count: manifest.payloads.len(),
        };
        atomic_write(&self.path.join("manifest.json"), canonical)?;
        atomic_write(
            &self.path.join("manifest.sig"),
            &serde_json::to_vec(&envelope).map_err(|_| RepositoryError::Serialization)?,
        )?;
        atomic_write(
            &self.path.join("reports/verification.json"),
            &serde_json::to_vec(&report).map_err(|_| RepositoryError::Serialization)?,
        )
    }

    fn publish_backup(&self, backup_id: &BackupId) -> Result<SealedBackup, RepositoryError> {
        let destination = self.repository.backups_root().join(backup_id.as_str());
        if destination.exists() {
            return Err(RepositoryError::BackupExists);
        }
        fs::rename(&self.path, &destination)
            .map_err(|source| RepositoryError::io("atomically seal backup", source))?;
        sync_parent(&destination)?;
        Ok(SealedBackup {
            backup_id: backup_id.clone(),
            path: destination,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedBackup {
    pub backup_id: BackupId,
    pub path: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VerificationReport {
    state: VerificationState,
    payload_count: usize,
}
