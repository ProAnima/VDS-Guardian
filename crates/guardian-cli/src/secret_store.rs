use guardian_core::{CredentialId, SecretStore, SecretStoreError, SecretValue};
use guardian_os_keyring::OsCredentialStore;
use guardian_vault::EncryptedFileVault;
use std::path::Path;

/// Selects which `SecretStore` backend a CLI invocation uses. Defaults to the
/// OS credential store; `--vault-dir` opts into the encrypted local vault
/// fallback explicitly. A vault that fails to open is a hard failure, never
/// a silent fallback to the OS store.
pub(crate) enum ResolvedStore {
    Os(OsCredentialStore),
    Vault(EncryptedFileVault),
}

impl SecretStore for ResolvedStore {
    fn load(&self, id: &CredentialId) -> Result<Option<SecretValue>, SecretStoreError> {
        match self {
            Self::Os(store) => store.load(id),
            Self::Vault(store) => store.load(id),
        }
    }

    fn store(&self, id: &CredentialId, secret: &SecretValue) -> Result<(), SecretStoreError> {
        match self {
            Self::Os(store) => store.store(id, secret),
            Self::Vault(store) => store.store(id, secret),
        }
    }

    fn delete(&self, id: &CredentialId) -> Result<(), SecretStoreError> {
        match self {
            Self::Os(store) => store.delete(id),
            Self::Vault(store) => store.delete(id),
        }
    }
}

pub(crate) fn resolve_store(vault_dir: Option<&Path>) -> Result<ResolvedStore, SecretStoreError> {
    match vault_dir {
        Some(dir) => EncryptedFileVault::open(dir)
            .map(ResolvedStore::Vault)
            .map_err(Into::into),
        None => Ok(ResolvedStore::Os(OsCredentialStore)),
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_store;
    use guardian_core::SecretStoreError;

    #[test]
    fn no_vault_dir_resolves_to_the_os_store() {
        assert!(matches!(
            resolve_store(None),
            Ok(super::ResolvedStore::Os(_))
        ));
    }

    #[test]
    fn an_uninitialized_vault_dir_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let vault_dir = root.path().join("vault");
        assert!(matches!(
            resolve_store(Some(&vault_dir)),
            Err(SecretStoreError::Unavailable)
        ));
        Ok(())
    }
}
