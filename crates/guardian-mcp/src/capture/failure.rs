//! Failures the capture tools report to an MCP client: a stable code and a message, never
//! internal detail. The mapping from capture errors lives here so it can be tested directly.

use guardian_core::CaptureUseCaseError;
use serde::Serialize;

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CaptureFailure {
    pub code: &'static str,
    pub message: &'static str,
}

impl CaptureFailure {
    pub(super) fn plan() -> Self {
        Self {
            code: "capture_plan_not_ready",
            message: "The capture plan, server, or repository is unavailable.",
        }
    }
    pub(super) fn signing() -> Self {
        Self {
            code: "signing_identity_unavailable",
            message: "This node has no ready signing identity to verify backups with.",
        }
    }
    pub(super) fn repository() -> Self {
        Self {
            code: "repository_unavailable",
            message: "The backup repository could not be opened.",
        }
    }
    pub(super) fn capture() -> Self {
        Self {
            code: "capture_failed",
            message: "The backup did not pass the verified capture lifecycle.",
        }
    }
    pub(super) fn insufficient_space() -> Self {
        Self {
            code: "repository_disk_space_low",
            message: "The repository disk does not have enough free space for this backup; free space or use a larger disk. Nothing was written.",
        }
    }
    pub(super) fn cancelled() -> Self {
        Self {
            code: "capture_cancelled",
            message: "The capture was cancelled by the operator.",
        }
    }
    pub(super) fn recovery_key_required() -> Self {
        Self {
            code: "recovery_key_not_configured",
            message: "This repository has no configured recovery key; run `recovery init` for it first.",
        }
    }
    pub(super) fn internal() -> Self {
        Self {
            code: "internal_error",
            message: "The capture request could not be processed.",
        }
    }
    pub(super) fn selection() -> Self {
        Self {
            code: "capture_selection_changed",
            message: "The selected server data changed or was not confirmed.",
        }
    }
}

/// Maps a failed capture to what the client is told. Cancellation is only reported when the
/// operator actually cancelled; specific, actionable causes win over the generic failure.
pub(super) fn capture_failure(error: &CaptureUseCaseError, cancelled: bool) -> CaptureFailure {
    match error {
        CaptureUseCaseError::RecoveryKeyRequired => CaptureFailure::recovery_key_required(),
        CaptureUseCaseError::InsufficientRepositorySpace { .. } => {
            CaptureFailure::insufficient_space()
        }
        _ if cancelled => CaptureFailure::cancelled(),
        _ => CaptureFailure::capture(),
    }
}

#[cfg(test)]
mod tests {
    use super::capture_failure;
    use guardian_core::CaptureUseCaseError;

    #[test]
    fn a_full_repository_disk_is_reported_as_disk_space_low() {
        let error = CaptureUseCaseError::InsufficientRepositorySpace {
            available_bytes: 13_600_000_000,
            required_bytes: 48_318_382_080,
        };
        for cancelled in [false, true] {
            let failure = capture_failure(&error, cancelled);
            assert_eq!(failure.code, "repository_disk_space_low");
            assert!(failure.message.contains("Nothing was written"));
        }
    }

    #[test]
    fn a_missing_recovery_key_is_named_and_other_failures_stay_generic() {
        let recovery = capture_failure(&CaptureUseCaseError::RecoveryKeyRequired, false);
        assert_eq!(recovery.code, "recovery_key_not_configured");
        let error = CaptureUseCaseError::RecoveryKeyRequired;
        assert_eq!(
            capture_failure(&error, true).code,
            "recovery_key_not_configured"
        );
        let generic = CaptureUseCaseError::Archive;
        assert_eq!(capture_failure(&generic, false).code, "capture_failed");
        assert_eq!(capture_failure(&generic, true).code, "capture_cancelled");
    }
}
