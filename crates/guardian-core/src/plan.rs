use crate::{PlanId, ProfileId, RepositoryId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesystemCapturePlan {
    pub plan_id: PlanId,
    pub version: u32,
    pub profile_id: ProfileId,
    pub repository_id: RepositoryId,
    pub roots: Vec<String>,
}

impl FilesystemCapturePlan {
    pub fn validate(&self) -> Result<(), CapturePlanError> {
        let roots_valid = !self.roots.is_empty()
            && self.roots.len() <= 32
            && self.roots.iter().all(|root| valid_remote_root(root));
        (self.version > 0 && roots_valid)
            .then_some(())
            .ok_or(CapturePlanError::Invalid)
    }

    pub fn canonical_sha256(&self) -> Result<String, CapturePlanError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| CapturePlanError::Serialization)?;
        Ok(hex(&Sha256::digest(bytes)))
    }
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

fn valid_remote_root(root: &str) -> bool {
    root == "/"
        || (root.starts_with('/')
            && root.len() <= 1_024
            && !root.contains(['\0', '\n', '\r', '\\'])
            && root
                .split('/')
                .skip(1)
                .all(|part| !matches!(part, "" | "." | "..")))
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CapturePlanError {
    #[error("capture plan roots or version are invalid")]
    Invalid,
    #[error("capture plan could not be serialized")]
    Serialization,
}
