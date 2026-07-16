use guardian_core::CredentialId;
use serde::Serialize;
use std::process::ExitCode;

#[derive(Serialize)]
#[serde(untagged)]
pub(super) enum RecoveryOutput {
    Init { credential_id: CredentialId },
    Status { configured: bool },
    Export { output: String },
    Import { credential_id: CredentialId },
}

pub(super) fn write_success(output: &RecoveryOutput) -> ExitCode {
    match serde_json::to_string_pretty(&Success {
        ok: true,
        data: output,
    }) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(_) => write_error(&RecoveryFailure::serialization()),
    }
}

pub(super) fn write_error(error: &RecoveryFailure) -> ExitCode {
    match serde_json::to_string_pretty(&Failure { ok: false, error }) {
        Ok(json) => eprintln!("{json}"),
        Err(_) => eprintln!("VDS Guardian could not serialize a redacted error."),
    }
    ExitCode::FAILURE
}

#[derive(Serialize)]
struct Success<'a, T> {
    ok: bool,
    data: &'a T,
}

#[derive(Serialize)]
struct Failure<'a, T> {
    ok: bool,
    error: &'a T,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RecoveryFailure {
    code: &'static str,
    message: &'static str,
    usage: &'static str,
}

impl RecoveryFailure {
    pub(super) fn usage() -> Self {
        Self {
            code: "invalid_arguments",
            message: "The command arguments are invalid.",
            usage: "guardian-cli recovery init --repositories-dir <absolute-path> --repository-id <id> --signing-config-dir <absolute-path> [--vault-dir <absolute-path>] --json | guardian-cli recovery status --repositories-dir <absolute-path> --repository-id <id> [--vault-dir <absolute-path>] --json | guardian-cli recovery export --repositories-dir <absolute-path> --repository-id <id> --passphrase-file <absolute-path> --output <absolute-path> --confirmation \"EXPORT RECOVERY BUNDLE FOR <id>\" [--vault-dir <absolute-path>] --json | guardian-cli recovery import --repositories-dir <absolute-path> --repository-id <id> [--repository-path <absolute-path>] --input <absolute-path> --passphrase-file <absolute-path> --confirmation \"IMPORT RECOVERY BUNDLE FOR <id>\" [--vault-dir <absolute-path>] --json",
        }
    }

    pub(super) fn input() -> Self {
        Self {
            code: "invalid_repository",
            message: "The repository id is not registered.",
            usage: "Register the repository first, then retry.",
        }
    }

    pub(super) fn store() -> Self {
        Self {
            code: "credential_store_unavailable",
            message: "The secure credential store could not complete the request.",
            usage: "Unlock or configure the operating-system credential store and retry.",
        }
    }

    pub(super) fn storage() -> Self {
        Self {
            code: "repository_storage_unavailable",
            message: "The repository could not be read.",
            usage: "Check local storage access and retry.",
        }
    }

    pub(super) fn signing() -> Self {
        Self {
            code: "signing_identity_unavailable",
            message: "The repository signing identity is not ready.",
            usage: "Enroll the signing identity, then retry recovery initialization.",
        }
    }

    pub(super) fn already_configured() -> Self {
        Self {
            code: "recovery_key_already_configured",
            message: "This repository already has a configured recovery key.",
            usage: "Recovery key rotation is not implemented yet; each repository keeps its original recovery key.",
        }
    }

    pub(super) fn not_configured() -> Self {
        Self {
            code: "recovery_key_not_configured",
            message: "This repository has no configured recovery key to export.",
            usage: "Run `recovery init` for this repository first.",
        }
    }

    pub(super) fn confirmation_mismatch() -> Self {
        Self {
            code: "confirmation_mismatch",
            message: "The confirmation phrase does not match this repository.",
            usage: "Pass the exact phrase, e.g. \"EXPORT RECOVERY BUNDLE FOR <repository-id>\" or \"IMPORT RECOVERY BUNDLE FOR <repository-id>\".",
        }
    }

    pub(super) fn passphrase_input() -> Self {
        Self {
            code: "invalid_passphrase_input",
            message: "The passphrase file is not a safe non-empty regular file.",
            usage: "Provide an absolute non-symlink file no larger than 4 KiB containing only the passphrase.",
        }
    }

    pub(super) fn bundle_io() -> Self {
        Self {
            code: "invalid_recovery_bundle",
            message: "The recovery bundle file could not be read or written.",
            usage: "For export, use a not-yet-existing absolute output path. For import, provide a bundle file this version of guardian-cli produced.",
        }
    }

    pub(super) fn bundle_operation() -> Self {
        Self {
            code: "recovery_bundle_operation_failed",
            message: "The recovery bundle could not be sealed or opened.",
            usage: "For import, check the passphrase and that the bundle matches this repository id; a wrong value fails closed without partial output.",
        }
    }

    pub(super) fn serialization() -> Self {
        Self {
            code: "serialization_failed",
            message: "The JSON response could not be serialized.",
            usage: "Retry and export redacted diagnostics if the problem persists.",
        }
    }
}
