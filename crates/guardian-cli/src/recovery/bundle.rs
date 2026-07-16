use super::RecoveryFailure;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use guardian_encryption::recovery_bundle::{KdfParams, SALT_BYTES, WrappedRecoveryKey};
use guardian_local_repository::RepositoryVerificationKey;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

#[cfg(test)]
mod tests;

const MAX_PASSPHRASE_FILE_BYTES: u64 = 4 * 1024;
const MAX_BUNDLE_FILE_BYTES: u64 = 16 * 1024;
const BUNDLE_FORMAT_VERSION: u32 = 1;
const BUNDLE_KDF: &str = "argon2id";

pub(super) fn read_passphrase(path: &Path) -> Result<Vec<u8>, RecoveryFailure> {
    let metadata = fs::symlink_metadata(path).map_err(|_| RecoveryFailure::passphrase_input())?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_PASSPHRASE_FILE_BYTES
    {
        return Err(RecoveryFailure::passphrase_input());
    }
    let bytes = fs::read(path).map_err(|_| RecoveryFailure::passphrase_input())?;
    let text = std::str::from_utf8(&bytes).map_err(|_| RecoveryFailure::passphrase_input())?;
    let trimmed = text.trim_end_matches(['\r', '\n']);
    if trimmed.is_empty() {
        return Err(RecoveryFailure::passphrase_input());
    }
    Ok(trimmed.as_bytes().to_vec())
}

pub(super) fn write_bundle_file(
    path: &Path,
    bundle: &RecoveryBundleFile,
) -> Result<(), RecoveryFailure> {
    let bytes = serde_json::to_vec_pretty(bundle).map_err(|_| RecoveryFailure::serialization())?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| RecoveryFailure::bundle_io())?;
    file.write_all(&bytes)
        .map_err(|_| RecoveryFailure::bundle_io())
}

pub(super) fn read_bundle_file(path: &Path) -> Result<RecoveryBundleFile, RecoveryFailure> {
    let metadata = fs::symlink_metadata(path).map_err(|_| RecoveryFailure::bundle_io())?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_BUNDLE_FILE_BYTES
    {
        return Err(RecoveryFailure::bundle_io());
    }
    let bytes = fs::read(path).map_err(|_| RecoveryFailure::bundle_io())?;
    serde_json::from_slice(&bytes).map_err(|_| RecoveryFailure::bundle_io())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RecoveryBundleFile {
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
    pub(super) fn from_wrapped(
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

    pub(super) fn verification_key(&self) -> RepositoryVerificationKey {
        self.verification_key.clone()
    }

    pub(super) fn kdf_params(&self) -> KdfParams {
        KdfParams {
            m_cost: self.m_cost,
            t_cost: self.t_cost,
            p_cost: self.p_cost,
        }
    }

    pub(super) fn to_wrapped(&self) -> Result<WrappedRecoveryKey, RecoveryFailure> {
        if self.format_version != BUNDLE_FORMAT_VERSION
            || self.kdf != BUNDLE_KDF
            || self.kdf_params() != KdfParams::recommended()
        {
            return Err(RecoveryFailure::bundle_io());
        }
        let salt_bytes = STANDARD
            .decode(&self.salt_base64)
            .map_err(|_| RecoveryFailure::bundle_io())?;
        let salt: [u8; SALT_BYTES] = salt_bytes
            .try_into()
            .map_err(|_| RecoveryFailure::bundle_io())?;
        let ciphertext = STANDARD
            .decode(&self.ciphertext_base64)
            .map_err(|_| RecoveryFailure::bundle_io())?;
        Ok(WrappedRecoveryKey { salt, ciphertext })
    }
}
