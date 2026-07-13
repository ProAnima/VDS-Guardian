use crate::{BackupId, PayloadPath, PlanId, ProfileId, RunId, Timestamp};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

pub const CURRENT_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format_version: u32,
    pub backup_id: BackupId,
    pub run_id: RunId,
    pub created_at: Timestamp,
    pub sealed_at: Option<Timestamp>,
    pub producer: Producer,
    pub source: SourceIdentity,
    pub plan: PlanReference,
    pub consistency: ConsistencyLevel,
    pub payloads: Vec<PayloadEntry>,
    pub verification_state: VerificationState,
    pub signature: Option<SignatureMetadata>,
    pub warnings: Vec<String>,
}

impl Manifest {
    #[must_use]
    pub fn new(
        backup_id: BackupId,
        run_id: RunId,
        created_at: Timestamp,
        producer: Producer,
        source: SourceIdentity,
        plan: PlanReference,
    ) -> Self {
        Self {
            format_version: CURRENT_FORMAT_VERSION,
            backup_id,
            run_id,
            created_at,
            sealed_at: None,
            producer,
            source,
            plan,
            consistency: ConsistencyLevel::CrashConsistent,
            payloads: Vec::new(),
            verification_state: VerificationState::Pending,
            signature: None,
            warnings: Vec::new(),
        }
    }

    pub fn add_payload(&mut self, payload: PayloadEntry) -> Result<(), ManifestError> {
        if self.payloads.iter().any(|entry| entry.path == payload.path) {
            return Err(ManifestError::DuplicatePayloadPath);
        }
        self.payloads.push(payload);
        Ok(())
    }

    pub fn prepare_for_seal(
        &mut self,
        sealed_at: Timestamp,
        algorithm: &str,
        key_id: &str,
    ) -> Result<(), ManifestError> {
        validate_label(algorithm)?;
        validate_label(key_id)?;
        self.validate_for_verification()?;
        self.sealed_at = Some(sealed_at);
        self.verification_state = VerificationState::Verified;
        self.signature = Some(SignatureMetadata {
            algorithm: algorithm.to_owned(),
            key_id: key_id.to_owned(),
        });
        Ok(())
    }

    pub fn validate_for_verification(&self) -> Result<(), ManifestError> {
        self.validate_common()?;
        if self.verification_state != VerificationState::Pending
            || self.sealed_at.is_some()
            || self.signature.is_some()
        {
            return Err(ManifestError::AlreadyFinalized);
        }
        Ok(())
    }

    pub fn validate_sealed(&self) -> Result<(), ManifestError> {
        self.validate_common()?;
        let signature = self.signature.as_ref().ok_or(ManifestError::NotSealed)?;
        if self.verification_state != VerificationState::Verified || self.sealed_at.is_none() {
            return Err(ManifestError::NotSealed);
        }
        validate_label(&signature.algorithm)?;
        validate_label(&signature.key_id)
    }

    fn validate_common(&self) -> Result<(), ManifestError> {
        if self.format_version != CURRENT_FORMAT_VERSION {
            return Err(ManifestError::UnsupportedFormatVersion);
        }
        if self.payloads.is_empty() {
            return Err(ManifestError::EmptyPayload);
        }
        for value in [
            self.producer.name.as_str(),
            self.producer.version.as_str(),
            self.producer.platform.as_str(),
            self.source.host_key_fingerprint.as_str(),
        ] {
            validate_label(value)?;
        }
        if self.plan.version == 0 || !is_sha256(&self.plan.sha256) {
            return Err(ManifestError::InvalidPlanReference);
        }
        for entry in &self.payloads {
            entry.validate()?;
        }
        for warning in &self.warnings {
            validate_label(warning)?;
        }
        let unique = self
            .payloads
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<HashSet<_>>();
        (unique.len() == self.payloads.len())
            .then_some(())
            .ok_or(ManifestError::DuplicatePayloadPath)
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ManifestError> {
        serde_json::to_vec(self).map_err(|_| ManifestError::Serialization)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Producer {
    pub name: String,
    pub version: String,
    pub platform: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceIdentity {
    pub profile_id: ProfileId,
    pub host_key_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanReference {
    pub plan_id: PlanId,
    pub version: u32,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsistencyLevel {
    CrashConsistent,
    ApplicationConsistent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayloadEntry {
    pub logical_role: String,
    pub path: PayloadPath,
    pub byte_length: u64,
    pub sha256: String,
    pub media_type: String,
}

impl PayloadEntry {
    pub fn new(
        logical_role: impl Into<String>,
        path: PayloadPath,
        byte_length: u64,
        sha256: impl Into<String>,
        media_type: impl Into<String>,
    ) -> Result<Self, ManifestError> {
        let entry = Self {
            logical_role: logical_role.into(),
            path,
            byte_length,
            sha256: sha256.into(),
            media_type: media_type.into(),
        };
        entry.validate()?;
        Ok(entry)
    }

    fn validate(&self) -> Result<(), ManifestError> {
        if !self.path.as_str().starts_with("payload/") {
            return Err(ManifestError::InvalidPayloadPath);
        }
        validate_label(&self.logical_role)?;
        validate_label(&self.media_type)?;
        is_sha256(&self.sha256)
            .then_some(())
            .ok_or(ManifestError::InvalidSha256)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureMetadata {
    pub algorithm: String,
    pub key_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationState {
    Pending,
    Verified,
}

fn validate_label(value: &str) -> Result<(), ManifestError> {
    (!value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control))
        .then_some(())
        .ok_or(ManifestError::InvalidLabel)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("manifest format version is unsupported")]
    UnsupportedFormatVersion,
    #[error("only a fresh pending manifest can enter verification")]
    AlreadyFinalized,
    #[error("manifest is not a complete verified sealed manifest")]
    NotSealed,
    #[error("manifest must contain at least one payload")]
    EmptyPayload,
    #[error("manifest payload paths must be unique")]
    DuplicatePayloadPath,
    #[error("manifest SHA-256 must contain exactly 64 hexadecimal characters")]
    InvalidSha256,
    #[error("manifest payload entries must remain under payload/")]
    InvalidPayloadPath,
    #[error("manifest plan version or digest is invalid")]
    InvalidPlanReference,
    #[error("manifest text field is empty, too long, or contains control characters")]
    InvalidLabel,
    #[error("manifest serialization failed")]
    Serialization,
}

#[cfg(test)]
mod tests {
    use super::{ManifestError, PayloadEntry};
    use crate::PayloadPath;

    #[test]
    fn payload_rejects_invalid_digest() -> Result<(), Box<dyn std::error::Error>> {
        let path = PayloadPath::parse("payload/fs.tar.zst")?;
        let result = PayloadEntry::new("filesystem", path, 10, "abcd", "application/zstd");
        assert_eq!(result, Err(ManifestError::InvalidSha256));
        Ok(())
    }
}
