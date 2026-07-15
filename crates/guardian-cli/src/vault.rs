use guardian_vault::{EncryptedFileVault, VaultInitOutcome, VaultStatus};
use serde::Serialize;
use std::{ffi::OsString, path::PathBuf, process::ExitCode};

pub(super) fn run(arguments: &[OsString]) -> ExitCode {
    match parse(arguments).and_then(execute) {
        Ok(output) => write_success(&output),
        Err(error) => write_error(&error),
    }
}

fn parse(arguments: &[OsString]) -> Result<VaultCommand, VaultCliFailure> {
    let action = match arguments.first().and_then(|value| value.to_str()) {
        Some("init") => VaultAction::Init,
        Some("status") => VaultAction::Status,
        _ => return Err(VaultCliFailure::usage()),
    };
    let mut vault_dir = None;
    let mut json = false;
    let mut index = 1;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--vault-dir") => {
                index += 1;
                vault_dir = arguments.get(index).map(PathBuf::from);
            }
            Some("--json") => json = true,
            _ => return Err(VaultCliFailure::usage()),
        }
        index += 1;
    }
    let vault_dir = vault_dir.ok_or_else(VaultCliFailure::usage)?;
    if !json || !vault_dir.is_absolute() {
        return Err(VaultCliFailure::usage());
    }
    Ok(VaultCommand { action, vault_dir })
}

fn execute(command: VaultCommand) -> Result<VaultOutput, VaultCliFailure> {
    match command.action {
        VaultAction::Init => EncryptedFileVault::init(&command.vault_dir)
            .map(VaultOutput::Init)
            .map_err(|_| VaultCliFailure::store()),
        VaultAction::Status => Ok(VaultOutput::Status(EncryptedFileVault::status(
            &command.vault_dir,
        ))),
    }
}

fn write_success(output: &VaultOutput) -> ExitCode {
    match serde_json::to_string_pretty(&Success {
        ok: true,
        data: output,
    }) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(_) => write_error(&VaultCliFailure::serialization()),
    }
}

fn write_error(error: &VaultCliFailure) -> ExitCode {
    match serde_json::to_string_pretty(&Failure { ok: false, error }) {
        Ok(json) => eprintln!("{json}"),
        Err(_) => eprintln!("VDS Guardian could not serialize a redacted error."),
    }
    ExitCode::FAILURE
}

struct VaultCommand {
    action: VaultAction,
    vault_dir: PathBuf,
}

#[derive(Clone, Copy)]
enum VaultAction {
    Init,
    Status,
}

#[derive(Serialize)]
#[serde(untagged)]
enum VaultOutput {
    Init(VaultInitOutcome),
    Status(VaultStatus),
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
struct VaultCliFailure {
    code: &'static str,
    message: &'static str,
    usage: &'static str,
}

impl VaultCliFailure {
    fn usage() -> Self {
        Self {
            code: "invalid_arguments",
            message: "The command arguments are invalid.",
            usage: "guardian-cli vault <init|status> --vault-dir <absolute-path> --json",
        }
    }

    fn store() -> Self {
        Self {
            code: "vault_operation_failed",
            message: "The vault operation could not be completed.",
            usage: "Check the vault directory and retry; run `vault status` for details.",
        }
    }

    fn serialization() -> Self {
        Self {
            code: "serialization_failed",
            message: "The JSON response could not be serialized.",
            usage: "Retry and export redacted diagnostics if the problem persists.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{VaultAction, VaultCliFailure, parse};
    use std::ffi::OsString;

    #[test]
    fn vault_commands_require_json_and_an_absolute_vault_dir() {
        for arguments in [
            vec!["init"],
            vec!["init", "--vault-dir", "relative", "--json"],
            vec!["status", "--vault-dir", "/v"],
            vec!["rotate", "--vault-dir", "/v", "--json"],
        ] {
            let values = arguments
                .into_iter()
                .map(OsString::from)
                .collect::<Vec<_>>();
            assert_eq!(parse(&values).err(), Some(VaultCliFailure::usage()));
        }
    }

    #[test]
    fn exact_vault_actions_are_distinct() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::current_dir()?;
        let options = |action: &str| {
            vec![
                OsString::from(action),
                OsString::from("--vault-dir"),
                root.as_os_str().to_owned(),
                OsString::from("--json"),
            ]
        };
        assert!(matches!(
            parse(&options("init")),
            Ok(command) if matches!(command.action, VaultAction::Init)
        ));
        assert!(matches!(
            parse(&options("status")),
            Ok(command) if matches!(command.action, VaultAction::Status)
        ));
        Ok(())
    }
}
