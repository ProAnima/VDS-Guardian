//! Binary secret storage in the platform credential manager.

use guardian_core::{CredentialId, SecretStore, SecretStoreError, SecretValue};
use keyring::v1::{Entry, Error};

const SERVICE_NAME: &str = "ProAnima.VDSGuardian";

#[derive(Debug, Default, Clone, Copy)]
pub struct OsCredentialStore;

impl SecretStore for OsCredentialStore {
    fn load(&self, id: &CredentialId) -> Result<Option<SecretValue>, SecretStoreError> {
        let entry = entry(id)?;
        match entry.get_secret() {
            Ok(secret) => Ok(Some(SecretValue::new(secret))),
            Err(Error::NoEntry) => Ok(None),
            Err(error) => Err(map_error(error)),
        }
    }

    fn store(&self, id: &CredentialId, secret: &SecretValue) -> Result<(), SecretStoreError> {
        entry(id)?.set_secret(secret.expose()).map_err(map_error)
    }

    fn delete(&self, id: &CredentialId) -> Result<(), SecretStoreError> {
        match entry(id)?.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(error) => Err(map_error(error)),
        }
    }
}

fn entry(id: &CredentialId) -> Result<Entry, SecretStoreError> {
    Entry::new(SERVICE_NAME, id.as_str()).map_err(map_error)
}

fn map_error(error: Error) -> SecretStoreError {
    match error {
        Error::NoStorageAccess(_) => SecretStoreError::AccessDenied,
        Error::NoDefaultStore | Error::NotSupportedByStore(_) => SecretStoreError::Unavailable,
        Error::BadEncoding(_) | Error::BadDataFormat(_, _) | Error::BadStoreFormat(_) => {
            SecretStoreError::InvalidData
        }
        _ => SecretStoreError::OperationFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::map_error;
    use guardian_core::SecretStoreError;
    use keyring::v1::Error;

    #[test]
    fn platform_error_mapping_discards_attached_secret_bytes() {
        let mapped = map_error(Error::BadEncoding(b"TOP-SECRET".to_vec()));
        assert_eq!(mapped, SecretStoreError::InvalidData);
        assert!(!format!("{mapped:?}").contains("TOP-SECRET"));
    }
}
