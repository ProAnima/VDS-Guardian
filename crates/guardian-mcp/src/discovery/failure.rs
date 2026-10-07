//! Failures the read-only discovery tools report: a stable code and a message, nothing internal.

use serde::Serialize;

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryFailure {
    pub code: &'static str,
    pub message: &'static str,
}

impl DiscoveryFailure {
    pub(super) fn storage() -> Self {
        Self {
            code: "storage_unavailable",
            message: "Local application storage could not be read.",
        }
    }
    pub(super) fn not_found() -> Self {
        Self {
            code: "not_found",
            message: "The requested profile or repository was not found.",
        }
    }
    pub(super) fn signing() -> Self {
        Self {
            code: "signing_identity_unavailable",
            message: "This node has no ready signing identity to verify backups with.",
        }
    }
    pub(super) fn inspection_failed() -> Self {
        Self {
            code: "docker_inspection_failed",
            message: "Could not read Docker containers from this server.",
        }
    }
    pub(super) fn browse_failed() -> Self {
        Self {
            code: "remote_browser_unavailable",
            message: "The requested server directory could not be read safely.",
        }
    }
    pub(super) fn rejected() -> Self {
        Self {
            code: "listing_rejected",
            message: "The repository's sealed backups could not be verified safely.",
        }
    }
}
