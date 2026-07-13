//! Platform-independent domain foundation for VDS Guardian.

use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Human-safe project readiness reported by every delivery surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundationStatus {
    pub product: String,
    pub version: String,
    pub iteration: String,
    pub live_operations_enabled: bool,
}

impl FoundationStatus {
    #[must_use]
    pub fn current() -> Self {
        Self {
            product: "VDS Guardian".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            iteration: "Iteration 0 — production foundation".to_owned(),
            live_operations_enabled: false,
        }
    }
}

/// Lifecycle states that may eventually be persisted in job events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupState {
    Planned,
    Staging,
    Captured,
    Verifying,
    Sealed,
    Failed,
    Cancelled,
    Quarantined,
}

impl BackupState {
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        use BackupState::{
            Cancelled, Captured, Failed, Planned, Quarantined, Sealed, Staging, Verifying,
        };

        matches!(
            (self, next),
            (Planned, Staging)
                | (Planned, Cancelled)
                | (Staging, Captured)
                | (Staging, Failed)
                | (Staging, Cancelled)
                | (Captured, Verifying)
                | (Captured, Quarantined)
                | (Verifying, Sealed)
                | (Verifying, Failed)
                | (Verifying, Quarantined)
        )
    }
}

/// Stable backup identifier. The final format will become an ADR-backed contract.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BackupId(String);

impl BackupId {
    pub fn parse(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));

        valid
            .then_some(Self(value))
            .ok_or(DomainError::InvalidBackupId)
    }
}

impl fmt::Display for BackupId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("backup ID must contain 1-64 ASCII letters, digits, '-' or '_'")]
    InvalidBackupId,
}

#[cfg(test)]
mod tests {
    use super::{BackupId, BackupState, FoundationStatus};

    #[test]
    fn foundation_disables_live_operations() {
        let status = FoundationStatus::current();
        assert!(!status.live_operations_enabled);
    }

    #[test]
    fn backup_can_only_seal_after_verification() {
        assert!(BackupState::Verifying.can_transition_to(BackupState::Sealed));
        assert!(!BackupState::Staging.can_transition_to(BackupState::Sealed));
        assert!(!BackupState::Failed.can_transition_to(BackupState::Sealed));
    }

    #[test]
    fn backup_id_rejects_path_syntax() {
        assert!(BackupId::parse("backup_01").is_ok());
        assert!(BackupId::parse("../escape").is_err());
        assert!(BackupId::parse("C:\\backup").is_err());
    }
}
