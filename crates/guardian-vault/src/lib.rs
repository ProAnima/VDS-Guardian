//! Encrypted local file vault: a `SecretStore` fallback for hosts without a
//! usable OS credential store (typically a headless Linux VDS with no
//! logged-in session bus for Secret Service). Selected explicitly per
//! invocation; never a silent runtime fallback from the OS store — see
//! `docs/adr/0006-headless-secret-vault.md`.

mod filesystem;
mod init;
mod master_key;
mod public;

use guardian_core::{CredentialId, SecretStore, SecretStoreError, SecretValue};
use guardian_encryption::PayloadKey;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub use init::VaultInitOutcome;
pub use public::{VaultErrorCode, VaultFailure, VaultState, VaultStatus};

const SECRET_DOMAIN: &[u8] = b"guardian-vault-secret-v1";
const MAX_SECRET_BYTES: usize = 64 * 1024;
const MAX_SECRET_ENVELOPE_BYTES: u64 = MAX_SECRET_BYTES as u64 + 4096;

pub struct EncryptedFileVault {
    root: PathBuf,
    key: PayloadKey,
}

impl EncryptedFileVault {
    /// Opens an existing vault. Fails closed as `NotInitialized` unless the
    /// directory, master key, and canary were already created by
    /// [`EncryptedFileVault::init`] — never creates anything as a side
    /// effect of opening.
    pub fn open(vault_dir: impl AsRef<Path>) -> Result<Self, VaultError> {
        let vault_dir = vault_dir.as_ref();
        filesystem::ensure_existing_directory(vault_dir)?;
        let key = master_key::load(vault_dir)?;
        master_key::verify_canary(vault_dir, &key)?;
        filesystem::ensure_existing_directory(&secrets_dir(vault_dir))?;
        Ok(Self {
            root: vault_dir.to_path_buf(),
            key,
        })
    }

    fn secret_path(&self, id: &CredentialId) -> PathBuf {
        secrets_dir(&self.root).join(format!("{}.enc", id.as_str()))
    }
}

impl SecretStore for EncryptedFileVault {
    fn load(&self, id: &CredentialId) -> Result<Option<SecretValue>, SecretStoreError> {
        load_secret(&self.key, &self.secret_path(id), id).map_err(Into::into)
    }

    fn store(&self, id: &CredentialId, secret: &SecretValue) -> Result<(), SecretStoreError> {
        store_secret(&self.key, &self.secret_path(id), id, secret).map_err(Into::into)
    }

    fn delete(&self, id: &CredentialId) -> Result<(), SecretStoreError> {
        filesystem::remove_regular_if_present(&self.secret_path(id)).map_err(Into::into)
    }
}

fn secrets_dir(root: &Path) -> PathBuf {
    root.join("secrets")
}

/// Binds each secret's ciphertext to its own credential id so a filesystem-
/// level attacker cannot silently swap two credentials' `.enc` files.
fn secret_aad(id: &CredentialId) -> Vec<u8> {
    let mut aad = Vec::with_capacity(SECRET_DOMAIN.len() + 1 + id.as_str().len());
    aad.extend_from_slice(SECRET_DOMAIN);
    aad.push(b'|');
    aad.extend_from_slice(id.as_str().as_bytes());
    aad
}

fn load_secret(
    key: &PayloadKey,
    path: &Path,
    id: &CredentialId,
) -> Result<Option<SecretValue>, VaultError> {
    let Some(bytes) = filesystem::read_file(path, MAX_SECRET_ENVELOPE_BYTES)? else {
        return Ok(None);
    };
    let mut plaintext = Vec::new();
    guardian_encryption::decrypt_self_describing_reader_to(
        key,
        &mut Cursor::new(bytes),
        &mut plaintext,
        &secret_aad(id),
    )
    .map_err(|_| VaultError::Corrupt)?;
    Ok(Some(SecretValue::new(plaintext)))
}

fn store_secret(
    key: &PayloadKey,
    path: &Path,
    id: &CredentialId,
    secret: &SecretValue,
) -> Result<(), VaultError> {
    if secret.expose().len() > MAX_SECRET_BYTES {
        return Err(VaultError::SecretTooLarge);
    }
    let mut ciphertext = Vec::new();
    guardian_encryption::encrypt_reader_to(
        key,
        &mut Cursor::new(secret.expose()),
        &mut ciphertext,
        &secret_aad(id),
    )
    .map_err(|_| VaultError::Encryption)?;
    filesystem::atomic_write(path, &ciphertext)
}

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("the vault is not initialized")]
    NotInitialized,
    #[error("the vault is already initialized")]
    AlreadyInitialized,
    #[error("the vault's stored key material is corrupt or tampered")]
    Corrupt,
    #[error("a vault operation is already running")]
    Busy,
    #[error("a secret exceeds the vault's size limit")]
    SecretTooLarge,
    #[error("the vault rejected an unsafe filesystem entry")]
    UnsafeFilesystemEntry,
    #[error("a vault cryptographic operation failed")]
    Encryption,
    #[error("vault I/O failed during {operation}")]
    Io {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
}

impl VaultError {
    pub(crate) fn io(operation: &'static str, source: std::io::Error) -> Self {
        Self::Io { operation, source }
    }
}

impl From<VaultError> for SecretStoreError {
    fn from(error: VaultError) -> Self {
        match error {
            VaultError::NotInitialized | VaultError::Busy => SecretStoreError::Unavailable,
            VaultError::Corrupt | VaultError::SecretTooLarge => SecretStoreError::InvalidData,
            VaultError::UnsafeFilesystemEntry
            | VaultError::AlreadyInitialized
            | VaultError::Encryption
            | VaultError::Io { .. } => SecretStoreError::OperationFailed,
        }
    }
}
