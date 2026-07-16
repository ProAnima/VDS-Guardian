use crate::{LocalRepository, RepositoryError, RepositoryVerificationKey};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::VerifyingKey;
use guardian_core::{CredentialId, RepositoryId, SecretStore};
use guardian_encryption::recovery_bundle::{self, KdfParams, SALT_BYTES, WrappedRecoveryKey};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};
use thiserror::Error;

const MAX_BUNDLE_FILE_BYTES: u64 = 16 * 1024;
const BUNDLE_FORMAT_VERSION: u32 = 1;
const BUNDLE_KDF: &str = "argon2id";

#[derive(Debug, Error)]
pub enum RecoveryBundleError {
    #[error("recovery-bundle confirmation does not match")]
    ConfirmationMismatch,
    #[error("repository recovery key is not configured")]
    NotConfigured,
    #[error("recovery bundle is invalid or unsupported")]
    InvalidBundle,
    #[error("recovery bundle cryptographic operation failed")]
    Crypto,
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    #[error("recovery bundle I/O failed")]
    Io,
}

pub fn export_confirmation_phrase(repository_id: &RepositoryId) -> String {
    format!("EXPORT RECOVERY BUNDLE FOR {}", repository_id.as_str())
}

pub fn import_confirmation_phrase(repository_id: &RepositoryId) -> String {
    format!("IMPORT RECOVERY BUNDLE FOR {}", repository_id.as_str())
}

pub fn export_recovery_bundle(
    repository: &LocalRepository,
    secrets: &dyn SecretStore,
    repository_id: &RepositoryId,
    passphrase: &[u8],
    output: &Path,
    confirmation: &str,
) -> Result<(), RecoveryBundleError> {
    if confirmation != export_confirmation_phrase(repository_id) {
        return Err(RecoveryBundleError::ConfirmationMismatch);
    }
    let key = repository
        .export_recovery_key(secrets)?
        .ok_or(RecoveryBundleError::NotConfigured)?;
    let verification_key = repository
        .trusted_verification_key()?
        .ok_or(RecoveryBundleError::InvalidBundle)?;
    validate_verification_key(&verification_key)?;
    let params = KdfParams::recommended();
    let wrapped = recovery_bundle::wrap_recovery_key(
        passphrase,
        &key,
        &bundle_binding(repository_id, &verification_key),
        params,
    )
    .map_err(|_| RecoveryBundleError::Crypto)?;
    write_bundle_file(
        output,
        &RecoveryBundleFile::from_wrapped(&wrapped, params, verification_key),
    )
}

pub fn import_recovery_bundle(
    repository: &LocalRepository,
    secrets: &dyn SecretStore,
    repository_id: &RepositoryId,
    passphrase: &[u8],
    input: &Path,
    confirmation: &str,
) -> Result<CredentialId, RecoveryBundleError> {
    if confirmation != import_confirmation_phrase(repository_id) {
        return Err(RecoveryBundleError::ConfirmationMismatch);
    }
    let bundle = read_bundle_file(input)?;
    let verification_key = bundle.verification_key.clone();
    validate_verification_key(&verification_key)?;
    if repository
        .trusted_verification_key()?
        .is_some_and(|existing| existing != verification_key)
    {
        return Err(RecoveryBundleError::InvalidBundle);
    }
    let key = recovery_bundle::unwrap_recovery_key(
        passphrase,
        &bundle.to_wrapped()?,
        &bundle_binding(repository_id, &verification_key),
        bundle.kdf_params(),
    )
    .map_err(|_| RecoveryBundleError::Crypto)?;
    let credential_id = repository.import_recovery_key(secrets, key)?;
    repository.pin_verification_key(verification_key)?;
    Ok(credential_id)
}

fn bundle_binding(repository_id: &RepositoryId, key: &RepositoryVerificationKey) -> String {
    format!(
        "{}|{}|{}|{}",
        repository_id.as_str(),
        key.algorithm,
        key.key_id,
        key.public_key_base64
    )
}

fn validate_verification_key(key: &RepositoryVerificationKey) -> Result<(), RecoveryBundleError> {
    if key.algorithm != "Ed25519" {
        return Err(RecoveryBundleError::InvalidBundle);
    }
    let bytes = STANDARD
        .decode(&key.public_key_base64)
        .map_err(|_| RecoveryBundleError::InvalidBundle)?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| RecoveryBundleError::InvalidBundle)?;
    let public_key =
        VerifyingKey::from_bytes(&bytes).map_err(|_| RecoveryBundleError::InvalidBundle)?;
    if key.key_id != public_key_id(public_key.as_bytes()) {
        return Err(RecoveryBundleError::InvalidBundle);
    }
    Ok(())
}

fn public_key_id(public_key: &[u8]) -> String {
    let digest = Sha256::digest(public_key);
    format!("ed25519:{}", hex(&digest))
}

fn hex(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(ALPHABET[usize::from(byte >> 4)]));
        output.push(char::from(ALPHABET[usize::from(byte & 0x0f)]));
    }
    output
}

fn write_bundle_file(path: &Path, bundle: &RecoveryBundleFile) -> Result<(), RecoveryBundleError> {
    let bytes =
        serde_json::to_vec_pretty(bundle).map_err(|_| RecoveryBundleError::InvalidBundle)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| RecoveryBundleError::Io)?;
    file.write_all(&bytes)
        .map_err(|_| RecoveryBundleError::Io)?;
    file.sync_all().map_err(|_| RecoveryBundleError::Io)
}

fn read_bundle_file(path: &Path) -> Result<RecoveryBundleFile, RecoveryBundleError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| RecoveryBundleError::Io)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_BUNDLE_FILE_BYTES
    {
        return Err(RecoveryBundleError::Io);
    }
    let bytes = fs::read(path).map_err(|_| RecoveryBundleError::Io)?;
    serde_json::from_slice(&bytes).map_err(|_| RecoveryBundleError::InvalidBundle)
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecoveryBundleFile {
    format_version: u32,
    kdf: String,
    m_cost: u32,
    t_cost: u32,
    p_cost: u32,
    salt_base64: String,
    ciphertext_base64: String,
    verification_key: RepositoryVerificationKey,
}

impl RecoveryBundleFile {
    fn from_wrapped(
        wrapped: &WrappedRecoveryKey,
        params: KdfParams,
        verification_key: RepositoryVerificationKey,
    ) -> Self {
        Self {
            format_version: BUNDLE_FORMAT_VERSION,
            kdf: BUNDLE_KDF.to_owned(),
            m_cost: params.m_cost,
            t_cost: params.t_cost,
            p_cost: params.p_cost,
            salt_base64: STANDARD.encode(wrapped.salt),
            ciphertext_base64: STANDARD.encode(&wrapped.ciphertext),
            verification_key,
        }
    }

    fn kdf_params(&self) -> KdfParams {
        KdfParams {
            m_cost: self.m_cost,
            t_cost: self.t_cost,
            p_cost: self.p_cost,
        }
    }

    fn to_wrapped(&self) -> Result<WrappedRecoveryKey, RecoveryBundleError> {
        if self.format_version != BUNDLE_FORMAT_VERSION
            || self.kdf != BUNDLE_KDF
            || self.kdf_params() != KdfParams::recommended()
        {
            return Err(RecoveryBundleError::InvalidBundle);
        }
        let salt = STANDARD
            .decode(&self.salt_base64)
            .map_err(|_| RecoveryBundleError::InvalidBundle)?;
        let salt: [u8; SALT_BYTES] = salt
            .try_into()
            .map_err(|_| RecoveryBundleError::InvalidBundle)?;
        let ciphertext = STANDARD
            .decode(&self.ciphertext_base64)
            .map_err(|_| RecoveryBundleError::InvalidBundle)?;
        Ok(WrappedRecoveryKey { salt, ciphertext })
    }
}

#[cfg(test)]
mod tests {
    use super::{RecoveryBundleError, RecoveryBundleFile};
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    #[test]
    fn bundle_rejects_untrusted_kdf_costs_before_key_derivation()
    -> Result<(), Box<dyn std::error::Error>> {
        let document = serde_json::json!({
            "formatVersion": 1,
            "kdf": "argon2id",
            "mCost": u32::MAX,
            "tCost": 3,
            "pCost": 4,
            "saltBase64": STANDARD.encode([0_u8; 16]),
            "ciphertextBase64": STANDARD.encode([0_u8; 32]),
            "verificationKey": {
                "algorithm": "Ed25519",
                "keyId": format!("ed25519:{}", "0".repeat(64)),
                "publicKeyBase64": STANDARD.encode([0_u8; 32])
            }
        });
        let bundle: RecoveryBundleFile = serde_json::from_value(document)?;
        assert!(matches!(
            bundle.to_wrapped(),
            Err(RecoveryBundleError::InvalidBundle)
        ));
        Ok(())
    }
}
