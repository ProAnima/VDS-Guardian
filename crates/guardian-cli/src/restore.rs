use guardian_configuration::RepositoryStore;
use guardian_core::{BackupId, RepositoryId, RestorePlan, SecretStore};
use guardian_local_repository::{LocalRepository, TrustedBackup};
use guardian_signing::SigningIdentityManager;
use serde::Serialize;
use std::{ffi::OsString, path::PathBuf, process::ExitCode};

pub(super) fn run(arguments: &[OsString], secrets: &dyn SecretStore) -> ExitCode {
    match parse(arguments).and_then(|command| execute(command, secrets)) {
        Ok(output) => write_success(&output),
        Err(error) => write_error(&error),
    }
}

fn parse(arguments: &[OsString]) -> Result<RestoreCommand, RestoreFailure> {
    let action = match arguments.first().and_then(|value| value.to_str()) {
        Some("list") => RestoreAction::List,
        Some("plan") => RestoreAction::Plan,
        Some("execute") => RestoreAction::Execute,
        _ => return Err(RestoreFailure::usage()),
    };
    let mut repositories_dir = None;
    let mut config_dir = None;
    let mut repository_id = None;
    let mut backup_id = None;
    let mut destination = None;
    let mut confirmation = None;
    let mut json = false;
    let mut index = 1;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--json") => json = true,
            Some("--repositories-dir") => {
                index += 1;
                repositories_dir = arguments.get(index).map(PathBuf::from);
            }
            Some("--config-dir") => {
                index += 1;
                config_dir = arguments.get(index).map(PathBuf::from);
            }
            Some("--repository-id") => {
                index += 1;
                repository_id = arguments.get(index).and_then(|value| value.to_str());
            }
            Some("--backup-id") if !matches!(action, RestoreAction::List) => {
                index += 1;
                backup_id = arguments.get(index).and_then(|value| value.to_str());
            }
            Some("--destination") if !matches!(action, RestoreAction::List) => {
                index += 1;
                destination = arguments.get(index).map(PathBuf::from);
            }
            Some("--confirmation") if matches!(action, RestoreAction::Execute) => {
                index += 1;
                confirmation = arguments.get(index).and_then(|value| value.to_str());
            }
            _ => return Err(RestoreFailure::usage()),
        }
        index += 1;
    }
    let repositories_dir = repositories_dir.ok_or_else(RestoreFailure::usage)?;
    let config_dir = config_dir.ok_or_else(RestoreFailure::usage)?;
    let repository_id = repository_id
        .map(str::to_owned)
        .ok_or_else(RestoreFailure::usage)?;
    if !json || !repositories_dir.is_absolute() || !config_dir.is_absolute() {
        return Err(RestoreFailure::usage());
    }
    let (backup_id, destination) = match action {
        RestoreAction::List => (None, None),
        RestoreAction::Plan | RestoreAction::Execute => {
            let backup_id = backup_id
                .map(str::to_owned)
                .ok_or_else(RestoreFailure::usage)?;
            let destination = destination.ok_or_else(RestoreFailure::usage)?;
            if !destination.is_absolute() {
                return Err(RestoreFailure::usage());
            }
            (Some(backup_id), Some(destination))
        }
    };
    let confirmation = confirmation.map(str::to_owned);
    if matches!(action, RestoreAction::Execute) && confirmation.is_none() {
        return Err(RestoreFailure::usage());
    }
    Ok(RestoreCommand {
        action,
        repositories_dir,
        config_dir,
        repository_id,
        backup_id,
        destination,
        confirmation,
    })
}

fn execute(
    command: RestoreCommand,
    secrets: &dyn SecretStore,
) -> Result<RestoreOutput, RestoreFailure> {
    let repository_id =
        RepositoryId::parse(&command.repository_id).map_err(|_| RestoreFailure::input())?;
    let registration = RepositoryStore::at(&command.repositories_dir)
        .get(&repository_id)
        .map_err(|_| RestoreFailure::storage())?
        .ok_or_else(RestoreFailure::input)?;
    let repository = LocalRepository::open(&registration.path, repository_id)
        .map_err(|_| RestoreFailure::storage())?;
    let identity = SigningIdentityManager::open(&command.config_dir)
        .map_err(|_| RestoreFailure::storage())?
        .load_ready(secrets)
        .map_err(|_| RestoreFailure::signing())?;
    match command.action {
        RestoreAction::List => repository
            .list_sealed_backups(&identity)
            .map(|inventory| {
                RestoreOutput::Backups(inventory.iter().map(BackupSummary::from).collect())
            })
            .map_err(|_| RestoreFailure::storage()),
        RestoreAction::Plan => {
            let backup_id = BackupId::parse(
                command
                    .backup_id
                    .as_deref()
                    .ok_or_else(RestoreFailure::usage)?,
            )
            .map_err(|_| RestoreFailure::input())?;
            let destination = command
                .destination
                .as_deref()
                .ok_or_else(RestoreFailure::usage)?;
            repository
                .plan_restore(&backup_id, destination, &identity)
                .map(|plan| RestoreOutput::Plan(RestorePlanSummary::from(&plan)))
                .map_err(|_| RestoreFailure::rejected())
        }
        RestoreAction::Execute => {
            let backup_id = BackupId::parse(
                command
                    .backup_id
                    .as_deref()
                    .ok_or_else(RestoreFailure::usage)?,
            )
            .map_err(|_| RestoreFailure::input())?;
            let destination = command
                .destination
                .as_deref()
                .ok_or_else(RestoreFailure::usage)?;
            let confirmation = command
                .confirmation
                .as_deref()
                .ok_or_else(RestoreFailure::usage)?;
            repository
                .execute_restore(&backup_id, destination, confirmation, &identity, secrets)
                .map(|plan| RestoreOutput::Plan(RestorePlanSummary::from(&plan)))
                .map_err(|_| RestoreFailure::rejected())
        }
    }
}

fn write_success(output: &RestoreOutput) -> ExitCode {
    match serde_json::to_string_pretty(&Success {
        ok: true,
        data: output,
    }) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(_) => write_error(&RestoreFailure::serialization()),
    }
}

fn write_error(error: &RestoreFailure) -> ExitCode {
    match serde_json::to_string_pretty(&Failure { ok: false, error }) {
        Ok(json) => eprintln!("{json}"),
        Err(_) => eprintln!("VDS Guardian could not serialize a redacted error."),
    }
    ExitCode::FAILURE
}

struct RestoreCommand {
    action: RestoreAction,
    repositories_dir: PathBuf,
    config_dir: PathBuf,
    repository_id: String,
    backup_id: Option<String>,
    destination: Option<PathBuf>,
    confirmation: Option<String>,
}

#[derive(Clone, Copy)]
enum RestoreAction {
    List,
    Plan,
    Execute,
}

#[derive(Serialize)]
#[serde(untagged)]
enum RestoreOutput {
    Backups(Vec<BackupSummary>),
    Plan(RestorePlanSummary),
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct BackupSummary {
    backup_id: String,
    sealed_at: String,
}

impl From<&TrustedBackup> for BackupSummary {
    fn from(value: &TrustedBackup) -> Self {
        Self {
            backup_id: value.backup_id.as_str().to_owned(),
            sealed_at: value.sealed_at.as_str().to_owned(),
        }
    }
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RestorePlanSummary {
    backup_id: String,
    destination: String,
    confirmation: String,
    payload: String,
}

impl From<&RestorePlan> for RestorePlanSummary {
    fn from(value: &RestorePlan) -> Self {
        Self {
            backup_id: value.backup_id.as_str().to_owned(),
            destination: value.destination.display().to_string(),
            confirmation: value.confirmation.clone(),
            payload: value.filesystem_payload.as_str().to_owned(),
        }
    }
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
struct RestoreFailure {
    code: &'static str,
    message: &'static str,
    usage: &'static str,
}

impl RestoreFailure {
    fn usage() -> Self {
        Self {
            code: "invalid_arguments",
            message: "The command arguments are invalid.",
            usage: "guardian-cli restore <list|plan|execute> --repositories-dir <absolute-path> --config-dir <absolute-path> --repository-id <id> [--backup-id <id> --destination <absolute-path>] [--confirmation <phrase>] --json",
        }
    }

    fn input() -> Self {
        Self {
            code: "invalid_restore_input",
            message: "The repository, backup, or destination is invalid.",
            usage: "Provide a registered repository id and, for plan/execute, a sealed backup id.",
        }
    }

    fn storage() -> Self {
        Self {
            code: "restore_storage_unavailable",
            message: "The repository or signing identity could not be read.",
            usage: "Check local storage access and the node signing identity, then retry.",
        }
    }

    fn signing() -> Self {
        Self {
            code: "restore_signing_identity_unavailable",
            message: "This node has no ready signing identity to verify backups with.",
            usage: "Run `guardian-cli signing enroll` on this node first.",
        }
    }

    fn rejected() -> Self {
        Self {
            code: "restore_rejected",
            message: "The restore could not be verified safely.",
            usage: "Use a sealed backup, a new absolute destination folder, and the exact confirmation phrase from `restore plan`.",
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
    use super::{RestoreAction, RestoreFailure, execute, parse};
    use guardian_core::{CredentialId, RepositoryId, SecretStore, SecretStoreError, SecretValue};
    use guardian_local_repository::LocalRepository;
    use guardian_signing::SigningIdentityManager;
    use std::{collections::HashMap, ffi::OsString, sync::Mutex};

    #[test]
    fn restore_commands_require_json_and_absolute_paths() {
        for arguments in [
            vec!["list"],
            vec![
                "list",
                "--repositories-dir",
                "relative",
                "--config-dir",
                "/n",
                "--repository-id",
                "r",
                "--json",
            ],
            vec![
                "plan",
                "--repositories-dir",
                "/r",
                "--config-dir",
                "/n",
                "--repository-id",
                "r",
                "--json",
            ],
            vec![
                "execute",
                "--repositories-dir",
                "/r",
                "--config-dir",
                "/n",
                "--repository-id",
                "r",
                "--backup-id",
                "b",
                "--destination",
                "/d",
                "--json",
            ],
        ] {
            let values = arguments
                .into_iter()
                .map(OsString::from)
                .collect::<Vec<_>>();
            assert_eq!(parse(&values).err(), Some(RestoreFailure::usage()));
        }
    }

    #[test]
    fn exact_restore_actions_are_distinct() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::current_dir()?;
        let options = |action: &str, extra: &[&str]| {
            let mut values = vec![
                action.to_owned(),
                "--repositories-dir".to_owned(),
                root.display().to_string(),
                "--config-dir".to_owned(),
                root.display().to_string(),
                "--repository-id".to_owned(),
                "repository-001".to_owned(),
            ];
            values.extend(extra.iter().map(|value| (*value).to_owned()));
            values.push("--json".to_owned());
            values.into_iter().map(OsString::from).collect::<Vec<_>>()
        };
        assert!(matches!(
            parse(&options("list", &[])),
            Ok(command) if matches!(command.action, RestoreAction::List)
        ));
        assert!(matches!(
            parse(&options(
                "plan",
                &["--backup-id", "backup-001", "--destination", &root.join("d").display().to_string()]
            )),
            Ok(command) if matches!(command.action, RestoreAction::Plan)
        ));
        Ok(())
    }

    #[test]
    fn listing_an_unregistered_repository_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let repositories_dir = root.path().join("repositories");
        std::fs::create_dir_all(&repositories_dir)?;
        let config_dir = root.path().join("node");
        SigningIdentityManager::open(&config_dir)?.enroll_or_load(&MemoryStore::default())?;
        let command = super::RestoreCommand {
            action: RestoreAction::List,
            repositories_dir,
            config_dir,
            repository_id: "repository-missing".to_owned(),
            backup_id: None,
            destination: None,
            confirmation: None,
        };
        let result = execute(command, &MemoryStore::default());
        assert_eq!(result.err(), Some(RestoreFailure::input()));
        Ok(())
    }

    #[test]
    fn listing_a_registered_repository_returns_its_sealed_backups()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let repositories_dir = root.path().join("repositories");
        let config_dir = root.path().join("node");
        let secrets = MemoryStore::default();
        let identity = SigningIdentityManager::open(&config_dir)?.enroll_or_load(&secrets)?;
        let repository_path = root.path().join("repository");
        let repository_id = RepositoryId::parse("repository-001")?;
        let repository = LocalRepository::open(&repository_path, repository_id.clone())?;
        drop(repository);
        std::fs::create_dir_all(&repositories_dir)?;
        guardian_configuration::RepositoryStore::at(&repositories_dir).upsert(
            guardian_configuration::RepositoryRegistration::new(
                repository_id,
                "Test repository".to_owned(),
                std::fs::canonicalize(&repository_path)?,
            )
            .map_err(|_| std::io::Error::other("invalid registration"))?,
        )?;
        drop(identity);
        let command = super::RestoreCommand {
            action: RestoreAction::List,
            repositories_dir,
            config_dir,
            repository_id: "repository-001".to_owned(),
            backup_id: None,
            destination: None,
            confirmation: None,
        };
        let output =
            execute(command, &secrets).map_err(|_| std::io::Error::other("restore list failed"))?;
        assert!(matches!(output, super::RestoreOutput::Backups(backups) if backups.is_empty()));
        Ok(())
    }

    #[derive(Default)]
    struct MemoryStore {
        values: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SecretStore for MemoryStore {
        fn load(&self, id: &CredentialId) -> Result<Option<SecretValue>, SecretStoreError> {
            let values = self
                .values
                .lock()
                .map_err(|_| SecretStoreError::OperationFailed)?;
            Ok(values.get(id.as_str()).cloned().map(SecretValue::new))
        }

        fn store(&self, id: &CredentialId, secret: &SecretValue) -> Result<(), SecretStoreError> {
            let mut values = self
                .values
                .lock()
                .map_err(|_| SecretStoreError::OperationFailed)?;
            values.insert(id.as_str().to_owned(), secret.expose().to_vec());
            Ok(())
        }

        fn delete(&self, id: &CredentialId) -> Result<(), SecretStoreError> {
            let mut values = self
                .values
                .lock()
                .map_err(|_| SecretStoreError::OperationFailed)?;
            values.remove(id.as_str());
            Ok(())
        }
    }
}
