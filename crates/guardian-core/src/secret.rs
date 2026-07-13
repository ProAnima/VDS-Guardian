use crate::CredentialId;
use thiserror::Error;
use zeroize::Zeroizing;

pub struct SecretValue(Zeroizing<Vec<u8>>);

impl SecretValue {
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(Zeroizing::new(bytes))
    }

    #[must_use]
    pub fn expose(&self) -> &[u8] {
        self.0.as_slice()
    }
}

pub trait SecretStore: Send + Sync {
    fn load(&self, id: &CredentialId) -> Result<Option<SecretValue>, SecretStoreError>;
    fn store(&self, id: &CredentialId, secret: &SecretValue) -> Result<(), SecretStoreError>;
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum SecretStoreError {
    #[error("secure credential storage is unavailable")]
    Unavailable,
    #[error("secure credential storage access was denied")]
    AccessDenied,
    #[error("secure credential storage returned invalid data")]
    InvalidData,
    #[error("secure credential storage operation failed")]
    OperationFailed,
}
