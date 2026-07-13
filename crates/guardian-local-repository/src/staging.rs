use crate::RepositoryError;
use crate::filesystem::{atomic_write, create_safe_parent, sync_parent, write_new};
use crate::repository::{LocalRepository, RepositoryLock};
use crate::verification::{hex, sha256_bytes, verify_payloads};
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
        if !relative_path.as_str().starts_with("payload/") {
            return Err(RepositoryError::UnsafeFilesystemEntry);
        }
        let entry = PayloadEntry::new(
            logical_role,
            relative_path.clone(),
            u64::try_from(bytes.len()).map_err(|_| RepositoryError::IntegrityFailure)?,
            sha256_bytes(bytes),
            media_type,
        )?;
        let target = create_safe_parent(&self.path, relative_path.as_str())?;
        write_new(&target, bytes)?;
        Ok(entry)
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
        verify_payloads(&self.path, manifest)?;
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
        let envelope = DiskSignature {
            algorithm: SIGNATURE_ALGORITHM,
            key_id: manifest
                .signature
                .as_ref()
                .ok_or(RepositoryError::IntegrityFailure)?
                .key_id
                .as_str(),
            signature: hex(signature),
        };
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
struct DiskSignature<'a> {
    algorithm: &'a str,
    key_id: &'a str,
    signature: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VerificationReport {
    state: VerificationState,
    payload_count: usize,
}
