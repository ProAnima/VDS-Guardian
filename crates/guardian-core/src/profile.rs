use crate::{CredentialId, ProfileId};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// A stable, displayable fingerprint for a pinned host key, shared by every
/// surface that needs to show or compare host identities (profile
/// enrollment UI, and the deploy same-source-host guard).
#[must_use]
pub fn host_key_fingerprint(public_key_base64: &str) -> String {
    format!("SHA256:{:x}", Sha256::digest(public_key_base64.as_bytes()))
}

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
    pub host_pin: HostPin,
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
        (host_valid && user_valid && self.port != 0)
            .then_some(())
            .ok_or(ProfileError::InvalidEndpoint)?;
        self.host_pin.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HostPin {
    pub algorithm: String,
    pub public_key_base64: String,
}

impl HostPin {
    pub fn parse(
        algorithm: impl Into<String>,
        public_key_base64: impl Into<String>,
    ) -> Result<Self, ProfileError> {
        let pin = Self {
            algorithm: algorithm.into(),
            public_key_base64: public_key_base64.into(),
        };
        pin.validate()?;
        Ok(pin)
    }
    pub fn validate(&self) -> Result<(), ProfileError> {
        let algorithm_valid = matches!(
            self.algorithm.as_str(),
            "ssh-ed25519" | "ecdsa-sha2-nistp256" | "ecdsa-sha2-nistp384" | "ecdsa-sha2-nistp521"
        );
        let decoded = STANDARD
            .decode(self.public_key_base64.as_bytes())
            .map_err(|_| ProfileError::InvalidHostPin)?;
        let length = decoded
            .get(..4)
            .map(|bytes| u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize)
            .ok_or(ProfileError::InvalidHostPin)?;
        (algorithm_valid
            && decoded.get(4..4 + length) == Some(self.algorithm.as_bytes())
            && decoded.len() > 4 + length)
            .then_some(())
            .ok_or(ProfileError::InvalidHostPin)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProfileError {
    #[error("profile label is invalid")]
    InvalidLabel,
    #[error("SSH endpoint or host pin is invalid")]
    InvalidEndpoint,
    #[error("SSH host pin is invalid")]
    InvalidHostPin,
}
