use crate::{BackupId, CredentialId, PayloadPath, PlanId, ProfileId, RunId, Timestamp};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

pub const CURRENT_FORMAT_VERSION: u32 = 1;
pub const ENCRYPTED_FORMAT_VERSION: u32 = 2;

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
        if payload.encryption.is_some() && self.format_version == CURRENT_FORMAT_VERSION {
            self.format_version = ENCRYPTED_FORMAT_VERSION;
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
        if !matches!(
            self.format_version,
            CURRENT_FORMAT_VERSION | ENCRYPTED_FORMAT_VERSION
        ) {
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
            match (self.format_version, entry.encryption.is_some()) {
                (CURRENT_FORMAT_VERSION, false) | (ENCRYPTED_FORMAT_VERSION, true) => {}
                _ => return Err(ManifestError::EncryptionPolicy),
            }
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
    /// The payload's size *as stored on disk at `path`* — the encrypted
    /// ciphertext size when `encryption` is `Some`, strictly larger than the
    /// plaintext it decrypts to. Checked byte-for-byte against the real file
    /// on every verification pass; not a stand-in for "how many bytes will a
    /// decrypted reader over this payload actually produce." A consumer that
    /// needs that number — deploy's push, for instance — must measure it
    /// from the decrypted content directly rather than reading this field.
    pub byte_length: u64,
    pub sha256: String,
    pub media_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption: Option<PayloadEncryption>,
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
            encryption: None,
        };
        entry.validate()?;
        Ok(entry)
    }

    pub fn encrypted(mut self, encryption: PayloadEncryption) -> Result<Self, ManifestError> {
        self.encryption = Some(encryption);
        self.validate()?;
        Ok(self)
    }

    fn validate(&self) -> Result<(), ManifestError> {
        if !self.path.as_str().starts_with("payload/") {
            return Err(ManifestError::InvalidPayloadPath);
        }
        validate_label(&self.logical_role)?;
        validate_label(&self.media_type)?;
        is_sha256(&self.sha256)
            .then_some(())
            .ok_or(ManifestError::InvalidSha256)?;
        if let Some(encryption) = &self.encryption {
            encryption.validate()?;
            self.path
                .as_str()
                .ends_with(".enc")
                .then_some(())
                .ok_or(ManifestError::EncryptionPolicy)?;
        }
        Ok(())
    }
}

/// Selects the single required filesystem payload and the optional database
/// payload from a manifest — shared by `RestorePlan::build` and
/// `DeploymentPlan::build`, which apply identical selection rules to two
/// otherwise-unrelated plan types.
pub(crate) fn select_payloads(
    manifest: &Manifest,
) -> Result<(PayloadPath, Option<PayloadPath>), PayloadSelectionError> {
    let mut filesystem_payloads = manifest
        .payloads
        .iter()
        .filter(|payload| payload.media_type == "application/zstd")
        .map(|payload| payload.path.clone())
        .collect::<Vec<_>>();
    if filesystem_payloads.len() != 1 {
        return Err(PayloadSelectionError::NoFilesystemPayload);
    }
    let mut database_payloads = manifest
        .payloads
        .iter()
        .filter(|payload| payload.logical_role == "database")
        .map(|payload| payload.path.clone())
        .collect::<Vec<_>>();
    if database_payloads.len() > 1 {
        return Err(PayloadSelectionError::AmbiguousDatabasePayload);
    }
    Ok((filesystem_payloads.remove(0), database_payloads.pop()))
}

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum PayloadSelectionError {
    #[error("backup has no supported filesystem payload")]
    NoFilesystemPayload,
    #[error("backup has more than one database payload")]
    AmbiguousDatabasePayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayloadEncryption {
    pub envelope_version: u8,
    pub algorithm: String,
    pub credential_id: CredentialId,
    pub nonce_base64: String,
    /// A second, independent copy of this payload's data key, wrapped under
    /// the repository's own recovery key (ADR 0013) via the same
    /// self-describing AEAD envelope `guardian-encryption` already uses for
    /// the vault's canary. Absent on any payload sealed before the recovery
    /// key existed, or in a repository that never configured one — restore
    /// then relies solely on the primary `SecretStore` entry, as before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_wrapped_key_base64: Option<String>,
}

impl PayloadEncryption {
    pub fn new(
        envelope_version: u8,
        algorithm: impl Into<String>,
        credential_id: CredentialId,
        nonce: &[u8; 12],
    ) -> Result<Self, ManifestError> {
        let encryption = Self {
            envelope_version,
            algorithm: algorithm.into(),
            credential_id,
            nonce_base64: STANDARD.encode(nonce),
            recovery_wrapped_key_base64: None,
        };
        encryption.validate()?;
        Ok(encryption)
    }

    /// Attaches a recovery-wrapped copy of this payload's data key, encoding
    /// the raw envelope ciphertext the same way `new` already encodes a raw
    /// nonce — the caller never handles base64 directly.
    pub fn with_recovery_wrapped_key(mut self, wrapped: &[u8]) -> Result<Self, ManifestError> {
        self.recovery_wrapped_key_base64 = Some(STANDARD.encode(wrapped));
        self.validate()?;
        Ok(self)
    }

    pub fn nonce(&self) -> Result<[u8; 12], ManifestError> {
        let bytes = STANDARD
            .decode(&self.nonce_base64)
            .map_err(|_| ManifestError::EncryptionPolicy)?;
        bytes
            .try_into()
            .map_err(|_| ManifestError::EncryptionPolicy)
    }

    /// Decodes the recovery-wrapped copy of this payload's data key, if
    /// present, sparing every caller from carrying its own base64
    /// dependency just to read one manifest field.
    pub fn recovery_wrapped_key(&self) -> Result<Option<Vec<u8>>, ManifestError> {
        match &self.recovery_wrapped_key_base64 {
            Some(value) => STANDARD
                .decode(value)
                .map(Some)
                .map_err(|_| ManifestError::EncryptionPolicy),
            None => Ok(None),
        }
    }

    fn validate(&self) -> Result<(), ManifestError> {
        (self.envelope_version == 1 && self.algorithm == "AES-256-GCM-CHUNKED")
            .then_some(())
            .ok_or(ManifestError::EncryptionPolicy)?;
        self.nonce()?;
        if let Some(wrapped) = self.recovery_wrapped_key()? {
            // guardian_encryption's self-describing envelope over a fixed
            // 32-byte plaintext (a PayloadKey) is always exactly 95 bytes:
            // an 8-byte magic + 1-byte version + 12-byte nonce (21-byte
            // header), one 32-byte data frame (1-byte final flag + 4-byte
            // length + 32-byte plaintext + 16-byte GCM tag = 53 bytes), and
            // one empty final frame (1 + 4 + 16-byte tag = 21 bytes).
            const WRAPPED_PAYLOAD_KEY_ENVELOPE_BYTES: usize = 95;
            (wrapped.len() == WRAPPED_PAYLOAD_KEY_ENVELOPE_BYTES)
                .then_some(())
                .ok_or(ManifestError::EncryptionPolicy)?;
        }
        Ok(())
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
    #[error("manifest encryption metadata or version policy is invalid")]
    EncryptionPolicy,
}

#[cfg(test)]
mod tests {
    use super::{CredentialId, ManifestError, PayloadEncryption, PayloadEntry};
    use crate::PayloadPath;

    #[test]
    fn payload_rejects_invalid_digest() -> Result<(), Box<dyn std::error::Error>> {
        let path = PayloadPath::parse("payload/fs.tar.zst")?;
        let result = PayloadEntry::new("filesystem", path, 10, "abcd", "application/zstd");
        assert_eq!(result, Err(ManifestError::InvalidSha256));
        Ok(())
    }

    #[test]
    fn recovery_wrapped_key_of_the_wrong_length_is_rejected()
    -> Result<(), Box<dyn std::error::Error>> {
        let encryption = PayloadEncryption::new(
            1,
            "AES-256-GCM-CHUNKED",
            CredentialId::parse("payload-0000")?,
            &[0_u8; 12],
        )?;
        let result = encryption.with_recovery_wrapped_key(&[0_u8; 42]);
        assert_eq!(result, Err(ManifestError::EncryptionPolicy));
        Ok(())
    }

    #[test]
    fn recovery_wrapped_key_of_exactly_95_bytes_is_accepted()
    -> Result<(), Box<dyn std::error::Error>> {
        let encryption = PayloadEncryption::new(
            1,
            "AES-256-GCM-CHUNKED",
            CredentialId::parse("payload-0000")?,
            &[0_u8; 12],
        )?;
        let result = encryption.with_recovery_wrapped_key(&[0_u8; 95]);
        assert!(result.is_ok());
        Ok(())
    }
}
