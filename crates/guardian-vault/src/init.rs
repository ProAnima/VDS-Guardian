use crate::{EncryptedFileVault, VaultError, filesystem, master_key, public};
use guardian_encryption::PayloadKey;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultInitOutcome {
    Created,
    Recovered,
}

impl EncryptedFileVault {
    /// Initializes a new vault, or recovers a previous `init` that wrote the
    /// master key but was interrupted before the canary was written. Never
    /// regenerates an existing master key — that would silently orphan every
    /// secret already encrypted under it.
    pub fn init(vault_dir: impl AsRef<Path>) -> Result<VaultInitOutcome, VaultError> {
        let vault_dir = vault_dir.as_ref();
        let _lock = filesystem::acquire_lock(vault_dir)?;
        filesystem::create_directory(vault_dir)?;
        filesystem::create_directory(&vault_dir.join("secrets"))?;
        match master_key::load(vault_dir) {
            Ok(key) => recover_or_reject(vault_dir, &key),
            Err(VaultError::NotInitialized) => create_fresh(vault_dir),
            Err(other) => Err(other),
        }
    }

    /// Read-only: reports whether the vault is initialized and structurally
    /// valid. Never creates the directory, key, or canary as a side effect.
    #[must_use]
    pub fn status(vault_dir: impl AsRef<Path>) -> public::VaultStatus {
        public::VaultStatus::from_open_result(Self::open(vault_dir.as_ref()))
    }
}

fn recover_or_reject(vault_dir: &Path, key: &PayloadKey) -> Result<VaultInitOutcome, VaultError> {
    match master_key::verify_canary(vault_dir, key) {
        Ok(()) => Err(VaultError::AlreadyInitialized),
        Err(VaultError::NotInitialized) => {
            master_key::write_canary(vault_dir, key)?;
            Ok(VaultInitOutcome::Recovered)
        }
        Err(other) => Err(other),
    }
}

fn create_fresh(vault_dir: &Path) -> Result<VaultInitOutcome, VaultError> {
    let key = PayloadKey::generate();
    master_key::write_new(vault_dir, &key)?;
    master_key::write_canary(vault_dir, &key)?;
    Ok(VaultInitOutcome::Created)
}
