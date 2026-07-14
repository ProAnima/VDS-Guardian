use crate::{CredentialId, ProfileId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct VdsProfile {
    pub profile_id: ProfileId,
    pub label: String,
    pub endpoint: SshEndpoint,
    pub credential_id: CredentialId,
}

impl VdsProfile {
    pub fn validate(&self) -> Result<(), ProfileError> {
        (!self.label.is_empty()
            && self.label.len() <= 128
            && !self.label.chars().any(char::is_control))
        .then_some(())
        .ok_or(ProfileError::InvalidLabel)?;
        self.endpoint.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SshEndpoint {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub host_key_algorithm: String,
    pub host_key_base64: String,
}

impl SshEndpoint {
    pub fn validate(&self) -> Result<(), ProfileError> {
        let host_valid = !self.host.is_empty()
            && self.host.len() <= 253
            && !self.host.starts_with(['-', '.'])
            && !self.host.ends_with(['-', '.'])
            && !self.host.contains("..")
            && self
                .host
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'));
        let user_valid = !self.user.is_empty()
            && self.user.len() <= 64
            && self
                .user
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
        let algorithm_valid = matches!(
            self.host_key_algorithm.as_str(),
            "ssh-ed25519" | "ecdsa-sha2-nistp256" | "ecdsa-sha2-nistp384" | "ecdsa-sha2-nistp521"
        );
        let key_valid = !self.host_key_base64.is_empty()
            && self.host_key_base64.len() <= 16_384
            && !self.host_key_base64.chars().any(char::is_whitespace);
        (host_valid && user_valid && self.port != 0 && algorithm_valid && key_valid)
            .then_some(())
            .ok_or(ProfileError::InvalidEndpoint)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProfileError {
    #[error("profile label is invalid")]
    InvalidLabel,
    #[error("SSH endpoint or host pin is invalid")]
    InvalidEndpoint,
}
