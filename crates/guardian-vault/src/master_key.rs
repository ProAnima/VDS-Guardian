use crate::{VaultError, filesystem};
use guardian_encryption::PayloadKey;
use std::io::Cursor;
use std::path::Path;

const KEY_FILE_NAME: &str = "vault.key";
const KEY_BYTES: usize = 32;
const CANARY_FILE_NAME: &str = "canary.enc";
const CANARY_AAD: &[u8] = b"guardian-vault-canary-v1";
const CANARY_PLAINTEXT: &[u8] = b"guardian-vault-canary";
const CANARY_MAX_BYTES: u64 = 4096;

pub(crate) fn load(vault_dir: &Path) -> Result<PayloadKey, VaultError> {
    let bytes = filesystem::read_file(&vault_dir.join(KEY_FILE_NAME), KEY_BYTES as u64)?
        .ok_or(VaultError::NotInitialized)?;
    if bytes.len() != KEY_BYTES {
        return Err(VaultError::Corrupt);
    }
    PayloadKey::from_bytes(&bytes).map_err(|_| VaultError::Corrupt)
}

/// Writes the master key once. Fails closed as `AlreadyInitialized` if a key
/// is already present — the caller (`EncryptedFileVault::init`) never
/// regenerates an existing key, which would silently orphan every secret
/// already encrypted under it.
pub(crate) fn write_new(vault_dir: &Path, key: &PayloadKey) -> Result<(), VaultError> {
    let path = vault_dir.join(KEY_FILE_NAME);
    if filesystem::exists(&path)? {
        return Err(VaultError::AlreadyInitialized);
    }
    filesystem::atomic_write(&path, key.expose())?;
    let confirmed = load(vault_dir)?;
    if confirmed.expose() != key.expose() {
        return Err(VaultError::Corrupt);
    }
    Ok(())
}

pub(crate) fn write_canary(vault_dir: &Path, key: &PayloadKey) -> Result<(), VaultError> {
    let mut ciphertext = Vec::new();
    guardian_encryption::encrypt_reader_to(
        key,
        &mut Cursor::new(CANARY_PLAINTEXT),
        &mut ciphertext,
        CANARY_AAD,
    )
    .map_err(|_| VaultError::Encryption)?;
    filesystem::atomic_write(&vault_dir.join(CANARY_FILE_NAME), &ciphertext)
}

/// Confirms the master key actually decrypts the fixed canary entry written
/// at `init` time — the only structural check available for an otherwise
/// opaque 32-byte key (for example after an operator copies the wrong node's
/// `vault.key` into place).
pub(crate) fn verify_canary(vault_dir: &Path, key: &PayloadKey) -> Result<(), VaultError> {
    let bytes = filesystem::read_file(&vault_dir.join(CANARY_FILE_NAME), CANARY_MAX_BYTES)?
        .ok_or(VaultError::NotInitialized)?;
    let mut plaintext = Vec::new();
    guardian_encryption::decrypt_self_describing_reader_to(
        key,
        &mut Cursor::new(bytes),
        &mut plaintext,
        CANARY_AAD,
    )
    .map_err(|_| VaultError::Corrupt)?;
    if plaintext != CANARY_PLAINTEXT {
        return Err(VaultError::Corrupt);
    }
    Ok(())
}
