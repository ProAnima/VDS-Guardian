use crate::{ALGORITHM, IdentityError};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, Verifier as _};
use guardian_core::{ManifestVerifier, SigningError};
use sha2::{Digest, Sha256};

const PUBLIC_KEY_LENGTH: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableVerificationKey {
    pub algorithm: String,
    pub key_id: String,
    pub public_key_base64: String,
}

impl PortableVerificationKey {
    pub(crate) fn from_public_key(key_id: String, public_key: &[u8]) -> Self {
        Self {
            algorithm: ALGORITHM.to_owned(),
            key_id,
            public_key_base64: STANDARD.encode(public_key),
        }
    }
}

pub struct Ed25519Verifier {
    key: ed25519_dalek::VerifyingKey,
    key_id: String,
}

impl Ed25519Verifier {
    pub fn from_portable(key: &PortableVerificationKey) -> Result<Self, IdentityError> {
        if key.algorithm != ALGORITHM {
            return Err(IdentityError::IncompatibleConfiguration);
        }
        let bytes = STANDARD
            .decode(&key.public_key_base64)
            .map_err(|_| IdentityError::IncompatibleConfiguration)?;
        let bytes = <[u8; PUBLIC_KEY_LENGTH]>::try_from(bytes)
            .map_err(|_| IdentityError::IncompatibleConfiguration)?;
        let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&bytes)
            .map_err(|_| IdentityError::IncompatibleConfiguration)?;
        let expected_key_id = public_key_id(verifying_key.as_bytes());
        if key.key_id != expected_key_id {
            return Err(IdentityError::ConfigurationMismatch);
        }
        Ok(Self {
            key: verifying_key,
            key_id: expected_key_id,
        })
    }
}

impl ManifestVerifier for Ed25519Verifier {
    fn verify_manifest(
        &self,
        algorithm: &str,
        key_id: &str,
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), SigningError> {
        if algorithm != ALGORITHM || key_id != self.key_id {
            return Err(SigningError::VerificationFailed);
        }
        let signature =
            Signature::from_slice(signature).map_err(|_| SigningError::VerificationFailed)?;
        self.key
            .verify(message, &signature)
            .map_err(|_| SigningError::VerificationFailed)
    }
}

pub(crate) fn public_key_id(public_key: &[u8]) -> String {
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
