//! Ed25519 identity lifecycle backed by an injected secure secret store.

mod enrollment;
mod filesystem;
mod public;

pub use enrollment::{ManagedIdentity, SigningIdentityManager};
pub use public::{
    EnrollmentDisposition, SigningIdentityDescriptor, SigningIdentityEnrollment,
    SigningIdentityErrorCode, SigningIdentityFailure, SigningIdentityState, SigningIdentityStatus,
};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier};
use guardian_core::{
    CredentialId, ManifestSigner, SecretStore, SecretStoreError, SecretValue, SigningError,
};
use rand_core::OsRng;
use sha2::{Digest, Sha256};
use thiserror::Error;
use zeroize::Zeroizing;

const ALGORITHM: &str = "Ed25519";
const SEED_LENGTH: usize = 32;

pub struct Ed25519Identity {
    signing_key: SigningKey,
    key_id: String,
}

impl Ed25519Identity {
    pub fn load(store: &dyn SecretStore, id: &CredentialId) -> Result<Self, IdentityError> {
        let secret = store.load(id)?.ok_or(IdentityError::Missing)?;
        Self::from_secret(&secret)
    }

    /// Enroll under a caller-held node configuration lock.
    pub fn enroll_exclusive(
        store: &dyn SecretStore,
        id: &CredentialId,
    ) -> Result<Self, IdentityError> {
        if store.load(id)?.is_some() {
            return Err(IdentityError::AlreadyEnrolled);
        }
        let generated = SigningKey::generate(&mut OsRng);
        let expected_public = generated.verifying_key().to_bytes();
        let secret = SecretValue::new(generated.to_bytes().to_vec());
        store.store(id, &secret)?;
        let persisted = Self::load(store, id)?;
        if persisted.signing_key.verifying_key().to_bytes() != expected_public {
            return Err(IdentityError::EnrollmentRace);
        }
        Ok(persisted)
    }

    fn from_secret(secret: &SecretValue) -> Result<Self, IdentityError> {
        let seed = <[u8; SEED_LENGTH]>::try_from(secret.expose())
            .map_err(|_| IdentityError::InvalidSecret)?;
        let seed = Zeroizing::new(seed);
        let signing_key = SigningKey::from_bytes(&seed);
        let key_id = key_id(&signing_key);
        Ok(Self {
            signing_key,
            key_id,
        })
    }
}

impl ManifestSigner for Ed25519Identity {
    fn algorithm(&self) -> &'static str {
        ALGORITHM
    }

    fn key_id(&self) -> &str {
        &self.key_id
    }

    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, SigningError> {
        Ok(self.signing_key.sign(message).to_bytes().to_vec())
    }

    fn verify(&self, message: &[u8], signature: &[u8]) -> Result<(), SigningError> {
        let signature =
            Signature::from_slice(signature).map_err(|_| SigningError::VerificationFailed)?;
        self.signing_key
            .verifying_key()
            .verify(message, &signature)
            .map_err(|_| SigningError::VerificationFailed)
    }
}

fn key_id(signing_key: &SigningKey) -> String {
    let digest = Sha256::digest(signing_key.verifying_key().as_bytes());
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

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("signing identity is not enrolled")]
    Missing,
    #[error("signing identity is already enrolled")]
    AlreadyEnrolled,
    #[error("stored signing identity is invalid")]
    InvalidSecret,
    #[error("signing identity changed during enrollment")]
    EnrollmentRace,
    #[error(transparent)]
    Store(#[from] SecretStoreError),
    #[error("signing enrollment is already running")]
    Busy,
    #[error("signing enrollment configuration is incompatible")]
    IncompatibleConfiguration,
    #[error("signing enrollment configuration does not match the secure identity")]
    ConfigurationMismatch,
    #[error("signing enrollment rejected an unsafe filesystem entry")]
    UnsafeFilesystemEntry,
    #[error("signing enrollment metadata serialization failed")]
    Serialization,
    #[error("signing enrollment I/O failed during {operation}")]
    Io {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
}

impl IdentityError {
    pub(crate) fn io(operation: &'static str, source: std::io::Error) -> Self {
        Self::Io { operation, source }
    }
}

#[cfg(test)]
mod tests {
    use super::{Ed25519Identity, IdentityError};
    use guardian_core::{
        CredentialId, ManifestSigner, SecretStore, SecretStoreError, SecretValue, SigningError,
    };
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[test]
    fn enrollment_persists_one_stable_identity() -> Result<(), Box<dyn std::error::Error>> {
        let store = MemoryStore::default();
        let id = CredentialId::parse("signing-main")?;
        let enrolled = Ed25519Identity::enroll_exclusive(&store, &id)?;
        let loaded = Ed25519Identity::load(&store, &id)?;
        assert_eq!(enrolled.key_id(), loaded.key_id());
        assert!(matches!(
            Ed25519Identity::enroll_exclusive(&store, &id),
            Err(IdentityError::AlreadyEnrolled)
        ));
        Ok(())
    }

    #[test]
    fn signatures_fail_closed_after_tampering() -> Result<(), Box<dyn std::error::Error>> {
        let store = MemoryStore::default();
        let id = CredentialId::parse("signing-main")?;
        let identity = Ed25519Identity::enroll_exclusive(&store, &id)?;
        let signature = identity.sign(b"manifest")?;
        assert!(identity.verify(b"manifest", &signature).is_ok());
        assert_eq!(
            identity.verify(b"tampered", &signature),
            Err(SigningError::VerificationFailed)
        );
        Ok(())
    }

    #[test]
    fn missing_and_malformed_secrets_have_redacted_errors() -> Result<(), Box<dyn std::error::Error>>
    {
        let store = MemoryStore::default();
        let missing = CredentialId::parse("missing")?;
        assert!(matches!(
            Ed25519Identity::load(&store, &missing),
            Err(IdentityError::Missing)
        ));
        store.store(&missing, &SecretValue::new(b"TOP-SECRET".to_vec()))?;
        let error = Ed25519Identity::load(&store, &missing).err();
        assert!(matches!(&error, Some(IdentityError::InvalidSecret)));
        assert!(!format!("{error:?}").contains("TOP-SECRET"));
        Ok(())
    }

    #[derive(Default)]
    struct MemoryStore {
        values: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SecretStore for MemoryStore {
        fn load(&self, id: &CredentialId) -> Result<Option<SecretValue>, SecretStoreError> {
            let values = self
                .values
                .lock()
                .map_err(|_| SecretStoreError::OperationFailed)?;
            Ok(values.get(id.as_str()).cloned().map(SecretValue::new))
        }

        fn store(&self, id: &CredentialId, secret: &SecretValue) -> Result<(), SecretStoreError> {
            let mut values = self
                .values
                .lock()
                .map_err(|_| SecretStoreError::OperationFailed)?;
            values.insert(id.as_str().to_owned(), secret.expose().to_vec());
            Ok(())
        }

        fn delete(&self, id: &CredentialId) -> Result<(), SecretStoreError> {
            let mut values = self
                .values
                .lock()
                .map_err(|_| SecretStoreError::OperationFailed)?;
            values.remove(id.as_str());
            Ok(())
        }
    }
}
