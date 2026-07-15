mod credential;
mod profile;
mod restore;
mod secret_store;
mod vault;

use guardian_core::{FoundationStatus, SecretStore};
use guardian_signing::{
    IdentityError, SigningIdentityEnrollment, SigningIdentityFailure, SigningIdentityManager,
    SigningIdentityStatus,
};
use secret_store::resolve_store;
use serde::Serialize;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

pub fn run(arguments: impl Iterator<Item = OsString>) -> ExitCode {
    let arguments: Vec<_> = arguments.collect();
    if arguments.first().and_then(|value| value.to_str()) == Some("profile") {
        return profile::run(&arguments[1..]);
    }
    if arguments.first().and_then(|value| value.to_str()) == Some("credential") {
        return credential::run(&arguments[1..]);
    }
    if arguments.first().and_then(|value| value.to_str()) == Some("restore") {
        return restore::run(&arguments[1..]);
    }
    if arguments.first().and_then(|value| value.to_str()) == Some("vault") {
        return vault::run(&arguments[1..]);
    }
    match parse(arguments) {
        Ok(Command::Foundation) => write_plain(&FoundationStatus::current()),
        Ok(Command::Signing(command)) => run_signing(command),
        Err(error) => write_error(&error, ExitCode::from(2)),
    }
}

fn write_plain(value: &impl Serialize) -> ExitCode {
    match serde_json::to_string_pretty(value) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(_) => write_error(&CliFailure::serialization(), ExitCode::FAILURE),
    }
}

fn run_signing(command: SigningCommand) -> ExitCode {
    let store = match resolve_store(command.vault_dir.as_deref()) {
        Ok(store) => store,
        Err(error) => {
            return write_error(
                &SigningIdentityFailure::from(IdentityError::Store(error)),
                ExitCode::FAILURE,
            );
        }
    };
    match execute_signing(command, &store) {
        Ok(SigningOutput::Status(status)) => write_success(&status),
        Ok(SigningOutput::Enrollment(enrollment)) => write_success(&enrollment),
        Err(error) => write_error(&error, ExitCode::FAILURE),
    }
}

fn execute_signing(
    command: SigningCommand,
    store: &dyn SecretStore,
) -> Result<SigningOutput, SigningIdentityFailure> {
    let manager =
        SigningIdentityManager::open(&command.config_dir).map_err(SigningIdentityFailure::from)?;
    match command.action {
        SigningAction::Status => manager
            .status(store)
            .map(SigningOutput::Status)
            .map_err(SigningIdentityFailure::from),
        SigningAction::Enroll => manager
            .enroll_or_load(store)
            .map(|identity| SigningOutput::Enrollment(identity.enrollment()))
            .map_err(SigningIdentityFailure::from),
    }
}

fn parse(arguments: Vec<OsString>) -> Result<Command, CliFailure> {
    if arguments.is_empty() {
        return Ok(Command::Foundation);
    }
    if arguments.first().and_then(|value| value.to_str()) != Some("signing") {
        return Err(CliFailure::usage());
    }
    let action = match arguments.get(1).and_then(|value| value.to_str()) {
        Some("status") => SigningAction::Status,
        Some("enroll") => SigningAction::Enroll,
        _ => return Err(CliFailure::usage()),
    };
    parse_signing_options(action, &arguments[2..])
}

fn parse_signing_options(
    action: SigningAction,
    options: &[OsString],
) -> Result<Command, CliFailure> {
    let mut config_dir = None;
    let mut vault_dir = None;
    let mut json = false;
    let mut index = 0;
    while index < options.len() {
        match options[index].to_str() {
            Some("--json") => json = true,
            Some("--config-dir") => {
                index += 1;
                config_dir = options.get(index).map(PathBuf::from);
            }
            Some("--vault-dir") => {
                index += 1;
                vault_dir = options.get(index).map(PathBuf::from);
            }
            _ => return Err(CliFailure::usage()),
        }
        index += 1;
    }
    let config_dir = config_dir.ok_or_else(CliFailure::usage)?;
    if !json
        || !config_dir.is_absolute()
        || vault_dir.as_deref().is_some_and(|dir| !dir.is_absolute())
    {
        return Err(CliFailure::usage());
    }
    Ok(Command::Signing(SigningCommand {
        action,
        config_dir,
        vault_dir,
    }))
}

fn write_success(value: &impl Serialize) -> ExitCode {
    match serde_json::to_string_pretty(&Success {
        ok: true,
        data: value,
    }) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(_) => write_error(&CliFailure::serialization(), ExitCode::FAILURE),
    }
}

fn write_error(value: &impl Serialize, exit_code: ExitCode) -> ExitCode {
    match serde_json::to_string_pretty(&Failure {
        ok: false,
        error: value,
    }) {
        Ok(json) => eprintln!("{json}"),
        Err(_) => eprintln!("VDS Guardian could not serialize a redacted error."),
    }
    exit_code
}

enum Command {
    Foundation,
    Signing(SigningCommand),
}

struct SigningCommand {
    action: SigningAction,
    config_dir: PathBuf,
    vault_dir: Option<PathBuf>,
}

#[derive(Clone, Copy)]
enum SigningAction {
    Status,
    Enroll,
}

enum SigningOutput {
    Status(SigningIdentityStatus),
    Enrollment(SigningIdentityEnrollment),
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
struct CliFailure {
    code: &'static str,
    message: &'static str,
    usage: &'static str,
}

impl CliFailure {
    fn usage() -> Self {
        Self {
            code: "invalid_arguments",
            message: "The command arguments are invalid.",
            usage: "guardian-cli signing <status|enroll> --config-dir <absolute-path> [--vault-dir <absolute-path>] --json",
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
    use super::{
        CliFailure, Command, SigningAction, SigningCommand, SigningOutput, execute_signing, parse,
    };
    use guardian_core::{CredentialId, SecretStore, SecretStoreError, SecretValue};
    use std::ffi::OsString;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn no_arguments_preserve_foundation_status() {
        assert!(matches!(parse(Vec::new()), Ok(Command::Foundation)));
    }

    #[test]
    fn enrollment_requires_explicit_json_command_and_absolute_path() {
        for arguments in [
            vec!["signing", "enroll"],
            vec!["signing", "enroll", "--config-dir", "relative", "--json"],
            vec!["signing", "status", "--config-dir", "/tmp/node"],
        ] {
            let parsed = parse(arguments.into_iter().map(OsString::from).collect());
            assert_eq!(parsed.err(), Some(CliFailure::usage()));
        }
    }

    #[test]
    fn exact_signing_actions_are_distinct() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::current_dir()?;
        let options = |action: &str| {
            vec![
                OsString::from("signing"),
                OsString::from(action),
                OsString::from("--config-dir"),
                root.as_os_str().to_owned(),
                OsString::from("--json"),
            ]
        };
        assert!(matches!(
            parse(options("status")),
            Ok(Command::Signing(command)) if matches!(command.action, SigningAction::Status)
        ));
        assert!(matches!(
            parse(options("enroll")),
            Ok(Command::Signing(command)) if matches!(command.action, SigningAction::Enroll)
        ));
        Ok(())
    }

    #[test]
    fn status_cannot_store_or_start_enrollment() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "vds-guardian-cli-status-test-{}",
            std::process::id()
        ));
        if root.exists() {
            std::fs::remove_dir_all(&root)?;
        }
        let store = CountingStore::default();
        let output = execute_signing(
            SigningCommand {
                action: SigningAction::Status,
                config_dir: root.clone(),
                vault_dir: None,
            },
            &store,
        )
        .map_err(|_| std::io::Error::other("signing status failed"))?;
        assert!(matches!(
            output,
            SigningOutput::Status(status) if status.identity.is_none()
        ));
        assert_eq!(store.stores.load(Ordering::Relaxed), 0);
        assert!(!root.join("signing-enrollment.json").exists());
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[derive(Default)]
    struct CountingStore {
        stores: AtomicUsize,
    }

    impl SecretStore for CountingStore {
        fn load(&self, _id: &CredentialId) -> Result<Option<SecretValue>, SecretStoreError> {
            Ok(None)
        }

        fn store(&self, _id: &CredentialId, _secret: &SecretValue) -> Result<(), SecretStoreError> {
            self.stores.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn delete(&self, _id: &CredentialId) -> Result<(), SecretStoreError> {
            Ok(())
        }
    }
}
