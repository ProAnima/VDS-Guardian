//! AES-256-GCM payload envelope primitives. Key persistence is delegated to a caller.

use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use rand_core::{OsRng, RngCore};
use thiserror::Error;
use zeroize::Zeroizing;

pub const ENVELOPE_VERSION: u8 = 1;
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;

pub struct PayloadKey(Zeroizing<[u8; KEY_BYTES]>);

impl PayloadKey {
    #[must_use]
    pub fn generate() -> Self {
        let mut key = [0_u8; KEY_BYTES];
        OsRng.fill_bytes(&mut key);
        Self(Zeroizing::new(key))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, EncryptionError> {
        let key: [u8; KEY_BYTES] = bytes.try_into().map_err(|_| EncryptionError::InvalidKey)?;
        Ok(Self(Zeroizing::new(key)))
    }

    #[must_use]
    pub fn expose(&self) -> &[u8; KEY_BYTES] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedPayload {
    pub version: u8,
    pub nonce: [u8; NONCE_BYTES],
    pub ciphertext: Vec<u8>,
}

pub fn encrypt(
    key: &PayloadKey,
    plaintext: &[u8],
    associated_data: &[u8],
) -> Result<EncryptedPayload, EncryptionError> {
    let mut nonce = [0_u8; NONCE_BYTES];
    OsRng.fill_bytes(&mut nonce);
    let cipher =
        Aes256Gcm::new_from_slice(key.expose()).map_err(|_| EncryptionError::InvalidKey)?;
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: associated_data,
            },
        )
        .map_err(|_| EncryptionError::Failed)?;
    Ok(EncryptedPayload {
        version: ENVELOPE_VERSION,
        nonce,
        ciphertext,
    })
}

pub fn decrypt(
    key: &PayloadKey,
    envelope: &EncryptedPayload,
    associated_data: &[u8],
) -> Result<Zeroizing<Vec<u8>>, EncryptionError> {
    if envelope.version != ENVELOPE_VERSION {
        return Err(EncryptionError::UnsupportedVersion);
    }
    let cipher =
        Aes256Gcm::new_from_slice(key.expose()).map_err(|_| EncryptionError::InvalidKey)?;
    cipher
        .decrypt(
            Nonce::from_slice(&envelope.nonce),
            Payload {
                msg: &envelope.ciphertext,
                aad: associated_data,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| EncryptionError::Failed)
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EncryptionError {
    #[error("payload key is invalid")]
    InvalidKey,
    #[error("payload envelope version is unsupported")]
    UnsupportedVersion,
    #[error("payload authentication failed")]
    Failed,
}

#[cfg(test)]
mod tests {
    use super::{EncryptedPayload, EncryptionError, PayloadKey, decrypt, encrypt};

    #[test]
    fn encryption_round_trip_authenticates_associated_data() -> Result<(), EncryptionError> {
        let key = PayloadKey::generate();
        let envelope = encrypt(&key, b"backup", b"backup-001|payload/filesystem.enc")?;
        assert_eq!(
            decrypt(&key, &envelope, b"backup-001|payload/filesystem.enc")?.as_slice(),
            b"backup"
        );
        assert!(matches!(
            decrypt(&key, &envelope, b"other"),
            Err(EncryptionError::Failed)
        ));
        Ok(())
    }

    #[test]
    fn altered_ciphertext_and_version_fail_closed() -> Result<(), EncryptionError> {
        let key = PayloadKey::generate();
        let mut envelope = encrypt(&key, b"backup", b"aad")?;
        envelope.ciphertext[0] ^= 1;
        assert!(matches!(
            decrypt(&key, &envelope, b"aad"),
            Err(EncryptionError::Failed)
        ));
        let unsupported = EncryptedPayload {
            version: 2,
            nonce: envelope.nonce,
            ciphertext: envelope.ciphertext,
        };
        assert!(matches!(
            decrypt(&key, &unsupported, b"aad"),
            Err(EncryptionError::UnsupportedVersion)
        ));
        Ok(())
    }
}
