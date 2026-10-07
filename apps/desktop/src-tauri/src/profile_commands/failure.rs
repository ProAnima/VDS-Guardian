//! Failures the server-profile commands report to the WebView: a stable code, a message, and
//! a remediation the operator can act on. None of them carries secret or transport detail.

use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileCommandFailure {
    pub code: &'static str,
    pub message: &'static str,
    pub remediation: &'static str,
}

impl ProfileCommandFailure {
    pub(super) fn invalid_profile() -> Self {
        Self {
            code: "invalid_profile",
            message: "The SSH profile is invalid.",
            remediation: "Check the server name, SSH address, user, port, and verified host key.",
        }
    }
    pub(super) fn invalid_key_path() -> Self {
        Self {
            code: "invalid_key_path",
            message: "The SSH key file is not a safe regular file.",
            remediation: "Use an absolute path to a non-symlink key file no larger than 64 KiB.",
        }
    }
    pub(super) fn invalid_key() -> Self {
        Self {
            code: "invalid_ssh_key",
            message: "The SSH key file is not a supported key.",
            remediation: "Choose a dedicated unencrypted OpenSSH or PEM private key, or the .pub file of an ed25519 or ECDSA key that is loaded in your SSH agent.",
        }
    }
    pub(super) fn host_key_unconfirmed() -> Self {
        Self {
            code: "host_key_unconfirmed",
            message: "The server's host key was not confirmed.",
            remediation: "Compare the host key or its fingerprint with a trusted source, then confirm it.",
        }
    }
    pub(super) fn host_key_mismatch() -> Self {
        Self {
            code: "host_key_fingerprint_mismatch",
            message: "The confirmed fingerprint does not belong to this host key.",
            remediation: "Fetch the host key again and compare the fingerprint that is shown.",
        }
    }
    pub(super) fn invalid_password() -> Self {
        Self {
            code: "invalid_login_password",
            message: "The login password cannot be used.",
            remediation: "Use 1 to 256 characters without line breaks.",
        }
    }
    pub(super) fn password_unavailable() -> Self {
        Self {
            code: "password_login_unavailable",
            message: "Password logins are not available in this installation.",
            remediation: "Reinstall the application, or use an SSH key instead.",
        }
    }
    pub(super) fn credential_store() -> Self {
        Self {
            code: "credential_store_unavailable",
            message: "The operating-system credential store could not save the SSH key.",
            remediation: "Unlock or configure the credential store and try again.",
        }
    }
    pub(super) fn storage() -> Self {
        Self {
            code: "profile_storage_unavailable",
            message: "The server profile could not be saved.",
            remediation: "Check local application storage and try again.",
        }
    }
    pub(super) fn preflight() -> Self {
        Self {
            code: "ssh_preflight_failed",
            message: "The server is not ready for a verified archive capture.",
            remediation: "Install GNU tar with zstd support for the backup account and recheck SSH access.",
        }
    }
    pub(super) fn internal() -> Self {
        Self {
            code: "internal_error",
            message: "The desktop command did not complete.",
            remediation: "Try again and export redacted diagnostics if the problem persists.",
        }
    }
}
