use crate::RepositoryError;
use crate::filesystem::{atomic_write, create_safe_parent, sync_parent, write_new};
use crate::repository::{LocalRepository, RepositoryLock, random_credential_id};
use crate::signature_file::DiskSignature;
use crate::verification::verify_staged_payloads;
use guardian_core::{
    BackupId, CredentialId, Manifest, ManifestSigner, PayloadEncryption, PayloadEntry, PayloadPath,
    RunId, SecretStore, SecretValue, Timestamp, VerificationState,
};
use guardian_encryption::{ALGORITHM, ENVELOPE_VERSION, PayloadKey, encrypt_reader_to};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use std::{fs, fs::OpenOptions, io::BufReader};

const SIGNATURE_ALGORITHM: &str = "Ed25519";

pub struct StagingBackup<'repository> {
    pub(crate) repository: &'repository LocalRepository,
    pub(crate) run_id: RunId,
    pub(crate) path: PathBuf,
    pub(crate) _lock: RepositoryLock,
    pub(crate) payload_credentials: Mutex<Vec<(CredentialId, &'repository dyn SecretStore)>>,
}

impl<'repository> StagingBackup<'repository> {
    pub fn discard(self) -> Result<(), RepositoryError> {
        let payload_root = self.path.join("payload");
        if payload_root.exists() {
            fs::remove_dir_all(&payload_root)
                .map_err(|source| RepositoryError::io("remove unsealed payloads", source))?;
        }
        self.delete_pending_credentials();
        self.repository.quarantine(&self.run_id, "capture_failed")?;
        Ok(())
    }

    /// Best-effort: revokes any payload encryption key registered so far in
    /// this run so a discarded or failed-to-seal capture never leaves a live,
    /// unreferenced key behind in the credential store.
    fn delete_pending_credentials(&self) {
        if let Ok(pending) = self.payload_credentials.lock() {
            for (id, secrets) in pending.iter() {
                let _ = secrets.delete(id);
            }
        }
    }

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

    pub fn encrypt_and_register_payload_file(
        &self,
        logical_role: impl Into<String>,
        relative_path: PayloadPath,
        media_type: impl Into<String>,
        backup_id: &BackupId,
        secrets: &'repository dyn SecretStore,
    ) -> Result<PayloadEntry, RepositoryError> {
        let target = self.path.join(relative_path.as_str());
        let metadata = fs::symlink_metadata(&target)
            .map_err(|source| RepositoryError::io("inspect plaintext staged payload", source))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(RepositoryError::UnsafeFilesystemEntry);
        }
        // Recovery wrapping (ADR 0013) is mandatory for every new payload,
        // checked before any bytes are touched: a repository that never
        // ran `recovery init` must fail closed here, not silently seal a
        // backup only the current OS keyring can ever decrypt.
        let recovery_key = self
            .repository
            .load_recovery_key(secrets)?
            .ok_or(RepositoryError::RecoveryKeyNotConfigured)?;
        let key = PayloadKey::generate();
        let credential_id = random_credential_id("payload")?;
        secrets
            .store(&credential_id, &SecretValue::new(key.expose().to_vec()))
            .map_err(|_| RepositoryError::Credential)?;
        if let Ok(mut pending) = self.payload_credentials.lock() {
            pending.push((credential_id.clone(), secrets));
        }
        let temporary = target.with_extension("encrypting");
        let header = match encrypt_payload(&target, &temporary, &key, backup_id, &relative_path) {
            Ok(header) => header,
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                return Err(error);
            }
        };
        fs::remove_file(&target)
            .map_err(|source| RepositoryError::io("remove plaintext staged payload", source))?;
        fs::rename(&temporary, &target)
            .map_err(|source| RepositoryError::io("publish encrypted staged payload", source))?;
        let wrapped_key = wrap_payload_key(&recovery_key, &key, backup_id, &relative_path)?;
        let encryption =
            PayloadEncryption::new(ENVELOPE_VERSION, ALGORITHM, credential_id, &header.nonce)?
                .with_recovery_wrapped_key(&wrapped_key)?;
        let entry = self.register_payload_file(logical_role, relative_path, media_type)?;
        Ok(entry.encrypted(encryption)?)
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
                self.delete_pending_credentials();
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

fn encrypt_payload(
    source: &std::path::Path,
    destination: &std::path::Path,
    key: &PayloadKey,
    backup_id: &BackupId,
    path: &PayloadPath,
) -> Result<guardian_encryption::EnvelopeHeader, RepositoryError> {
    let mut input = BufReader::new(
        fs::File::open(source)
            .map_err(|error| RepositoryError::io("open plaintext staged payload", error))?,
    );
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| RepositoryError::io("create encrypted staged payload", error))?;
    let aad = associated_data(backup_id, path);
    let header = encrypt_reader_to(key, &mut input, &mut output, &aad)
        .map_err(|_| RepositoryError::Encryption)?;
    output
        .sync_all()
        .map_err(|error| RepositoryError::io("sync encrypted staged payload", error))?;
    Ok(header)
}

/// Wraps a payload's data key under the repository recovery key (ADR 0013),
/// via the same self-describing AEAD envelope `guardian-vault`'s own canary
/// already uses for a small fixed-size secret — no new crypto primitive.
fn wrap_payload_key(
    recovery_key: &PayloadKey,
    payload_key: &PayloadKey,
    backup_id: &BackupId,
    path: &PayloadPath,
) -> Result<Vec<u8>, RepositoryError> {
    let mut ciphertext = Vec::new();
    encrypt_reader_to(
        recovery_key,
        &mut std::io::Cursor::new(payload_key.expose()),
        &mut ciphertext,
        &recovery_wrap_associated_data(backup_id, path),
    )
    .map_err(|_| RepositoryError::Encryption)?;
    Ok(ciphertext)
}

pub(crate) fn associated_data(backup_id: &BackupId, path: &PayloadPath) -> Vec<u8> {
    format!(
        "{}|{}|{}",
        backup_id.as_str(),
        path.as_str(),
        ENVELOPE_VERSION
    )
    .into_bytes()
}

/// Associated data for wrapping a payload's data key under the repository
/// recovery key (ADR 0013) — distinct from `associated_data` (the payload
/// stream's own AAD) so a wrapped-key ciphertext can never be silently
/// swapped between two payloads even if the two AADs otherwise collided.
pub(crate) fn recovery_wrap_associated_data(backup_id: &BackupId, path: &PayloadPath) -> Vec<u8> {
    format!(
        "guardian-recovery-wrap-v1|{}|{}",
        backup_id.as_str(),
        path.as_str()
    )
    .into_bytes()
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
