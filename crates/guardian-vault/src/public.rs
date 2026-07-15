use crate::{EncryptedFileVault, VaultError};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultState {
    NotInitialized,
    Ready,
    Corrupt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub state: VaultState,
}

impl VaultStatus {
    pub(crate) fn from_open_result(result: Result<EncryptedFileVault, VaultError>) -> Self {
        let state = match result {
            Ok(_) => VaultState::Ready,
            Err(VaultError::NotInitialized) => VaultState::NotInitialized,
            Err(_) => VaultState::Corrupt,
        };
        Self { state }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultFailure {
    pub code: VaultErrorCode,
    pub message: &'static str,
    pub remediation: &'static str,
}

impl From<VaultError> for VaultFailure {
    fn from(error: VaultError) -> Self {
        let code = error_code(&error);
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
pub enum VaultErrorCode {
    NotInitialized,
    AlreadyInitialized,
    Corrupt,
    Busy,
    SecretTooLarge,
    UnsafeFilesystemEntry,
    EncryptionFailed,
    LocalIoFailure,
}

fn error_code(error: &VaultError) -> VaultErrorCode {
    match error {
        VaultError::NotInitialized => VaultErrorCode::NotInitialized,
        VaultError::AlreadyInitialized => VaultErrorCode::AlreadyInitialized,
        VaultError::Corrupt => VaultErrorCode::Corrupt,
        VaultError::Busy => VaultErrorCode::Busy,
        VaultError::SecretTooLarge => VaultErrorCode::SecretTooLarge,
        VaultError::UnsafeFilesystemEntry => VaultErrorCode::UnsafeFilesystemEntry,
        VaultError::Encryption => VaultErrorCode::EncryptionFailed,
        VaultError::Io { .. } => VaultErrorCode::LocalIoFailure,
    }
}

fn error_text(code: VaultErrorCode) -> (&'static str, &'static str) {
    match code {
        VaultErrorCode::NotInitialized => (
            "The vault is not initialized.",
            "Run the explicit vault init command.",
        ),
        VaultErrorCode::AlreadyInitialized => (
            "The vault is already initialized.",
            "Use the existing vault; explicit rotation is not implemented yet.",
        ),
        VaultErrorCode::Corrupt => (
            "The vault's stored key material is corrupt or tampered.",
            "Stop and inspect the vault directory; do not reinitialize implicitly.",
        ),
        VaultErrorCode::Busy => (
            "The vault is busy with another operation.",
            "Wait for the other VDS Guardian process and retry.",
        ),
        VaultErrorCode::SecretTooLarge => (
            "The secret exceeds the vault's size limit.",
            "Store a smaller credential; the vault accepts at most 64 KiB per secret.",
        ),
        VaultErrorCode::UnsafeFilesystemEntry => (
            "The vault directory contains an unsafe filesystem entry.",
            "Remove links or special files from the vault directory.",
        ),
        VaultErrorCode::EncryptionFailed => (
            "A vault cryptographic operation failed.",
            "Retry and export redacted diagnostics if the problem persists.",
        ),
        VaultErrorCode::LocalIoFailure => (
            "The vault could not be accessed safely.",
            "Check local permissions and free space, then retry.",
        ),
    }
}
