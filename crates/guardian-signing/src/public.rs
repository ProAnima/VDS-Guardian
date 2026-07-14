use crate::IdentityError;
use guardian_core::{CredentialId, SecretStoreError};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SigningIdentityStatus {
    pub state: SigningIdentityState,
    pub identity: Option<SigningIdentityDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SigningIdentityEnrollment {
    pub disposition: EnrollmentDisposition,
    pub identity: SigningIdentityDescriptor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SigningIdentityDescriptor {
    pub credential_id: CredentialId,
    pub algorithm: String,
    pub key_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SigningIdentityState {
    NotEnrolled,
    EnrollmentPending,
    RecoveryPending,
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnrollmentDisposition {
    Enrolled,
    Recovered,
    Loaded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SigningIdentityFailure {
    pub code: SigningIdentityErrorCode,
    pub message: &'static str,
    pub remediation: &'static str,
}

impl From<IdentityError> for SigningIdentityFailure {
    fn from(error: IdentityError) -> Self {
        let code = error_code(&error);
        let (message, remediation) = error_text(code);
        Self {
            code,
            message,
            remediation,
        }
    }
}

impl SigningIdentityFailure {
    #[must_use]
    pub fn local_io() -> Self {
        let code = SigningIdentityErrorCode::LocalIoFailure;
        let (message, remediation) = error_text(code);
        Self {
            code,
            message,
            remediation,
        }
    }

    #[must_use]
    pub fn internal() -> Self {
        let code = SigningIdentityErrorCode::InternalFailure;
        let (message, remediation) = error_text(code);
        Self {
            code,
            message,
            remediation,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SigningIdentityErrorCode {
    NotEnrolled,
    Busy,
    CredentialStoreUnavailable,
    CredentialStoreDenied,
    InvalidCredential,
    IncompatibleConfiguration,
    ConfigurationMismatch,
    UnsafeFilesystemEntry,
    LocalIoFailure,
    InternalFailure,
}

fn error_code(error: &IdentityError) -> SigningIdentityErrorCode {
    match error {
        IdentityError::Missing => SigningIdentityErrorCode::NotEnrolled,
        IdentityError::Busy => SigningIdentityErrorCode::Busy,
        IdentityError::InvalidSecret | IdentityError::EnrollmentRace => {
            SigningIdentityErrorCode::InvalidCredential
        }
        IdentityError::Store(store) => store_error_code(*store),
        IdentityError::IncompatibleConfiguration => {
            SigningIdentityErrorCode::IncompatibleConfiguration
        }
        IdentityError::ConfigurationMismatch | IdentityError::AlreadyEnrolled => {
            SigningIdentityErrorCode::ConfigurationMismatch
        }
        IdentityError::UnsafeFilesystemEntry => SigningIdentityErrorCode::UnsafeFilesystemEntry,
        IdentityError::Io { .. } => SigningIdentityErrorCode::LocalIoFailure,
        IdentityError::Serialization => SigningIdentityErrorCode::InternalFailure,
    }
}

fn store_error_code(error: SecretStoreError) -> SigningIdentityErrorCode {
    match error {
        SecretStoreError::Unavailable => SigningIdentityErrorCode::CredentialStoreUnavailable,
        SecretStoreError::AccessDenied => SigningIdentityErrorCode::CredentialStoreDenied,
        SecretStoreError::InvalidData => SigningIdentityErrorCode::InvalidCredential,
        SecretStoreError::OperationFailed => SigningIdentityErrorCode::InternalFailure,
    }
}

fn error_text(code: SigningIdentityErrorCode) -> (&'static str, &'static str) {
    match code {
        SigningIdentityErrorCode::NotEnrolled => (
            "The signing identity is not enrolled.",
            "Run the explicit signing enrollment command.",
        ),
        SigningIdentityErrorCode::Busy => (
            "Signing identity configuration is busy.",
            "Wait for the other VDS Guardian process and retry.",
        ),
        SigningIdentityErrorCode::CredentialStoreUnavailable => (
            "Secure credential storage is unavailable.",
            "Unlock or configure the operating-system credential store.",
        ),
        SigningIdentityErrorCode::CredentialStoreDenied => (
            "Secure credential storage denied access.",
            "Grant access in the operating system and retry.",
        ),
        SigningIdentityErrorCode::InvalidCredential => (
            "The stored signing identity is invalid.",
            "Stop and inspect the local credential store; do not rotate implicitly.",
        ),
        SigningIdentityErrorCode::IncompatibleConfiguration => (
            "Signing identity configuration is incompatible.",
            "Restore a supported configuration or migrate it explicitly.",
        ),
        SigningIdentityErrorCode::ConfigurationMismatch => (
            "Signing configuration does not match the stored identity.",
            "Stop and reconcile the credential reference before continuing.",
        ),
        SigningIdentityErrorCode::UnsafeFilesystemEntry => (
            "Signing configuration contains an unsafe filesystem entry.",
            "Remove links or special files from the configuration directory.",
        ),
        SigningIdentityErrorCode::LocalIoFailure => (
            "Signing configuration could not be accessed safely.",
            "Check local permissions and free space, then retry.",
        ),
        SigningIdentityErrorCode::InternalFailure => (
            "Signing identity operation failed.",
            "Retry and export redacted diagnostics if the problem persists.",
        ),
    }
}
