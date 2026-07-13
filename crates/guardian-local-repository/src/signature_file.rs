use crate::RepositoryError;
use crate::verification::hex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiskSignature {
    pub algorithm: String,
    pub key_id: String,
    pub signature: String,
}

impl DiskSignature {
    pub fn new(algorithm: &str, key_id: &str, signature: &[u8]) -> Self {
        Self {
            algorithm: algorithm.to_owned(),
            key_id: key_id.to_owned(),
            signature: hex(signature),
        }
    }

    pub fn decode(&self) -> Result<Vec<u8>, RepositoryError> {
        if !self.signature.len().is_multiple_of(2) {
            return Err(RepositoryError::IntegrityFailure);
        }
        self.signature
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let text =
                    std::str::from_utf8(pair).map_err(|_| RepositoryError::IntegrityFailure)?;
                u8::from_str_radix(text, 16).map_err(|_| RepositoryError::IntegrityFailure)
            })
            .collect()
    }
}
