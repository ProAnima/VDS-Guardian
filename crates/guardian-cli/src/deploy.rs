use crate::secret_store::resolve_store;
use guardian_configuration::RepositoryStore;
use guardian_core::{
    BackupId, DeploymentPlan, ProfileId, ProfileStorePort, RemoteTargetPath, RunId, SecretStore,
    VdsProfile,
};
use guardian_deploy::DeploymentComposition;
use guardian_local_repository::LocalRepository;
use guardian_profile_store::ProfileStore;
use guardian_signing::SigningIdentityManager;
use guardian_ssh::SystemOpenSsh;
use rand_core::{OsRng, RngCore};
use serde::Serialize;
use std::{ffi::OsString, path::PathBuf, process::ExitCode};

pub(super) fn run(arguments: &[OsString]) -> ExitCode {
    match parse(arguments).and_then(|command| {
        let store =
            resolve_store(command.vault_dir.as_deref()).map_err(|_| DeployFailure::storage())?;
        execute(command, &store)
    }) {
        Ok(output) => write_success(&output),
        Err(error) => write_error(&error),
    }
}

fn parse(arguments: &[OsString]) -> Result<DeployCommand, DeployFailure> {
    let action = match arguments.first().and_then(|value| value.to_str()) {
        Some("plan") => DeployAction::Plan,
        Some("execute") => DeployAction::Execute,
        _ => return Err(DeployFailure::usage()),
    };
    let mut repositories_dir = None;
    let mut config_dir = None;
    let mut repository_id = None;
    let mut backup_id = None;
    let mut profiles_dir = None;
    let mut target_profile_id = None;
    let mut target_path = None;
    let mut confirmation = None;
    let mut vault_dir = None;
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
            Some("--backup-id") => {
                index += 1;
                backup_id = arguments.get(index).and_then(|value| value.to_str());
            }
            Some("--profiles-dir") => {
                index += 1;
                profiles_dir = arguments.get(index).map(PathBuf::from);
            }
            Some("--target-profile-id") => {
                index += 1;
                target_profile_id = arguments.get(index).and_then(|value| value.to_str());
            }
            Some("--target-path") => {
                index += 1;
                target_path = arguments.get(index).and_then(|value| value.to_str());
            }
            Some("--confirmation") if matches!(action, DeployAction::Execute) => {
                index += 1;
                confirmation = arguments.get(index).and_then(|value| value.to_str());
            }
            Some("--vault-dir") => {
                index += 1;
                vault_dir = arguments.get(index).map(PathBuf::from);
            }
            _ => return Err(DeployFailure::usage()),
        }
        index += 1;
    }
    let repositories_dir = repositories_dir.ok_or_else(DeployFailure::usage)?;
    let config_dir = config_dir.ok_or_else(DeployFailure::usage)?;
    let profiles_dir = profiles_dir.ok_or_else(DeployFailure::usage)?;
    let repository_id = repository_id
        .map(str::to_owned)
        .ok_or_else(DeployFailure::usage)?;
    let backup_id = backup_id
        .map(str::to_owned)
        .ok_or_else(DeployFailure::usage)?;
    let target_profile_id = target_profile_id
        .map(str::to_owned)
        .ok_or_else(DeployFailure::usage)?;
    let target_path = target_path
        .ok_or_else(DeployFailure::usage)
        .and_then(|value| RemoteTargetPath::parse(value).map_err(|_| DeployFailure::usage()))?;
    if !json
        || !repositories_dir.is_absolute()
        || !config_dir.is_absolute()
        || !profiles_dir.is_absolute()
        || vault_dir.as_deref().is_some_and(|dir| !dir.is_absolute())
    {
        return Err(DeployFailure::usage());
    }
    let confirmation = confirmation.map(str::to_owned);
    if matches!(action, DeployAction::Execute) && confirmation.is_none() {
        return Err(DeployFailure::usage());
    }
    Ok(DeployCommand {
        action,
        repositories_dir,
        config_dir,
        repository_id,
        backup_id,
        profiles_dir,
        target_profile_id,
        target_path,
        confirmation,
        vault_dir,
    })
}

fn execute(
    command: DeployCommand,
    secrets: &dyn SecretStore,
) -> Result<DeploymentPlanSummary, DeployFailure> {
    let repository_id = guardian_core::RepositoryId::parse(&command.repository_id)
        .map_err(|_| DeployFailure::input())?;
    let registration = RepositoryStore::at(&command.repositories_dir)
        .get(&repository_id)
        .map_err(|_| DeployFailure::storage())?
        .ok_or_else(DeployFailure::input)?;
    let repository = LocalRepository::open(&registration.path, repository_id)
        .map_err(|_| DeployFailure::storage())?;
    let identity = SigningIdentityManager::open(&command.config_dir)
        .map_err(|_| DeployFailure::storage())?
        .load_ready(secrets)
        .map_err(|_| DeployFailure::signing())?;
    let backup_id = BackupId::parse(&command.backup_id).map_err(|_| DeployFailure::input())?;
    let target_profile_id =
        ProfileId::parse(&command.target_profile_id).map_err(|_| DeployFailure::input())?;
    let target_profile = ProfileStore::at(&command.profiles_dir)
        .get(&target_profile_id)
        .map_err(|_| DeployFailure::storage())?
        .ok_or_else(DeployFailure::input)?;
    let target_path = command.target_path;
    let ssh = SystemOpenSsh::default();
    let composition = DeploymentComposition {
        repository: &repository,
        ssh: &ssh,
        target_profile: &target_profile,
        credentials: secrets,
        verifier: &identity,
    };
    match command.action {
        DeployAction::Plan => composition
            .plan(&backup_id, target_path)
            .map(|plan| summarize(&plan, &target_profile))
            .map_err(|_| DeployFailure::rejected()),
        DeployAction::Execute => execute_deploy(
            &repository,
            &composition,
            &backup_id,
            &target_profile_id,
            &target_profile,
            target_path,
            command.confirmation.as_deref(),
        ),
    }
}

fn execute_deploy(
    repository: &LocalRepository,
    composition: &DeploymentComposition<'_>,
    backup_id: &BackupId,
    target_profile_id: &ProfileId,
    target_profile: &VdsProfile,
    target_path: RemoteTargetPath,
    confirmation: Option<&str>,
) -> Result<DeploymentPlanSummary, DeployFailure> {
    let confirmation = confirmation.ok_or_else(DeployFailure::usage)?;
    let run_id = random_run_id()?;
    repository
        .write_deploy_audit(&run_id, "attempted", backup_id, target_profile_id)
        .map_err(|_| DeployFailure::storage())?;
    match composition.execute(target_profile_id, backup_id, target_path, confirmation) {
        Ok(plan) => {
            repository
                .write_deploy_audit(&run_id, "completed", backup_id, target_profile_id)
                .map_err(|_| DeployFailure::storage())?;
            Ok(summarize(&plan, target_profile))
        }
        Err(_) => {
            let _ = repository.write_deploy_audit(&run_id, "failed", backup_id, target_profile_id);
            Err(DeployFailure::rejected())
        }
    }
}

fn random_run_id() -> Result<RunId, DeployFailure> {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    let suffix = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    RunId::parse(format!("deploy-{suffix}")).map_err(|_| DeployFailure::storage())
}

fn summarize(plan: &DeploymentPlan, target_profile: &VdsProfile) -> DeploymentPlanSummary {
    DeploymentPlanSummary {
        backup_id: plan.backup_id.as_str().to_owned(),
        target_profile_id: plan.target_profile_id.as_str().to_owned(),
        target_profile_label: target_profile.label.clone(),
        target_path: plan.target_path.as_str().to_owned(),
        confirmation: plan.confirmation.clone(),
        filesystem_payload: plan.filesystem_payload.as_str().to_owned(),
        database_payload: plan
            .database_payload
            .as_ref()
            .map(|path| path.as_str().to_owned()),
    }
}

fn write_success(output: &DeploymentPlanSummary) -> ExitCode {
    match serde_json::to_string_pretty(&Success {
        ok: true,
        data: output,
    }) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(_) => write_error(&DeployFailure::serialization()),
    }
}

fn write_error(error: &DeployFailure) -> ExitCode {
    match serde_json::to_string_pretty(&Failure { ok: false, error }) {
        Ok(json) => eprintln!("{json}"),
        Err(_) => eprintln!("VDS Guardian could not serialize a redacted error."),
    }
    ExitCode::FAILURE
}

struct DeployCommand {
    action: DeployAction,
    repositories_dir: PathBuf,
    config_dir: PathBuf,
    repository_id: String,
    backup_id: String,
    profiles_dir: PathBuf,
    target_profile_id: String,
    target_path: RemoteTargetPath,
    confirmation: Option<String>,
    vault_dir: Option<PathBuf>,
}

#[derive(Clone, Copy)]
enum DeployAction {
    Plan,
    Execute,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct DeploymentPlanSummary {
    backup_id: String,
    target_profile_id: String,
    target_profile_label: String,
    target_path: String,
    confirmation: String,
    filesystem_payload: String,
    database_payload: Option<String>,
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
struct DeployFailure {
    code: &'static str,
    message: &'static str,
    usage: &'static str,
}

impl DeployFailure {
    fn usage() -> Self {
        Self {
            code: "invalid_arguments",
            message: "The command arguments are invalid.",
            usage: "guardian-cli deploy <plan|execute> --repositories-dir <absolute-path> --config-dir <absolute-path> --repository-id <id> --backup-id <id> --profiles-dir <absolute-path> --target-profile-id <id> --target-path <remote-absolute-path> [--confirmation <phrase>] [--vault-dir <absolute-path>] --json",
        }
    }

    fn input() -> Self {
        Self {
            code: "invalid_deploy_input",
            message: "The repository, backup, target profile, or target path is invalid.",
            usage: "Provide a registered repository id, a sealed backup id, and a registered target profile id with an absolute remote target path.",
        }
    }

    fn storage() -> Self {
        Self {
            code: "deploy_storage_unavailable",
            message: "The repository, profile store, or signing identity could not be read.",
            usage: "Check local storage access and the node signing identity, then retry.",
        }
    }

    fn signing() -> Self {
        Self {
            code: "deploy_signing_identity_unavailable",
            message: "This node has no ready signing identity to verify backups with.",
            usage: "Run `guardian-cli signing enroll` on this node first.",
        }
    }

    fn rejected() -> Self {
        Self {
            code: "deploy_rejected",
            message: "The deployment could not be verified or pushed safely.",
            usage: "Use a sealed backup, a different target profile with an absolute, currently-absent remote path, and the exact confirmation phrase from `deploy plan`.",
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
    use super::{DeployAction, DeployFailure, execute, parse};
    use guardian_signing::SigningIdentityManager;
    use std::ffi::OsString;

    #[test]
    fn deploy_commands_require_json_and_absolute_paths() {
        for arguments in [
            vec!["plan"],
            vec![
                "plan",
                "--repositories-dir",
                "relative",
                "--config-dir",
                "/n",
                "--repository-id",
                "r",
                "--backup-id",
                "b",
                "--profiles-dir",
                "/p",
                "--target-profile-id",
                "t",
                "--target-path",
                "/srv/app",
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
                "--profiles-dir",
                "/p",
                "--target-profile-id",
                "t",
                "--target-path",
                "/srv/app",
                "--json",
            ],
        ] {
            let values = arguments
                .into_iter()
                .map(OsString::from)
                .collect::<Vec<_>>();
            assert_eq!(parse(&values).err(), Some(DeployFailure::usage()));
        }
    }

    #[test]
    fn target_path_is_validated_as_posix_regardless_of_host_os()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::current_dir()?;
        let options = |target_path: &str| {
            vec![
                "plan".to_owned(),
                "--repositories-dir".to_owned(),
                root.display().to_string(),
                "--config-dir".to_owned(),
                root.display().to_string(),
                "--repository-id".to_owned(),
                "repository-001".to_owned(),
                "--backup-id".to_owned(),
                "backup-001".to_owned(),
                "--profiles-dir".to_owned(),
                root.display().to_string(),
                "--target-profile-id".to_owned(),
                "profile-target".to_owned(),
                "--target-path".to_owned(),
                target_path.to_owned(),
                "--json".to_owned(),
            ]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>()
        };
        // A Windows-style absolute path must never be accepted as a remote
        // target -- the remote host is always POSIX, regardless of which OS
        // guardian-cli itself runs on.
        assert_eq!(
            parse(&options(r"C:\srv\app")).err(),
            Some(DeployFailure::usage())
        );
        assert!(parse(&options("/srv/app")).is_ok());
        Ok(())
    }

    #[test]
    fn exact_deploy_actions_are_distinct() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::current_dir()?;
        let options = |action: &str| {
            vec![
                OsString::from(action),
                OsString::from("--repositories-dir"),
                root.as_os_str().to_owned(),
                OsString::from("--config-dir"),
                root.as_os_str().to_owned(),
                OsString::from("--repository-id"),
                OsString::from("repository-001"),
                OsString::from("--backup-id"),
                OsString::from("backup-001"),
                OsString::from("--profiles-dir"),
                root.as_os_str().to_owned(),
                OsString::from("--target-profile-id"),
                OsString::from("profile-target"),
                OsString::from("--target-path"),
                OsString::from("/srv/app"),
                OsString::from("--json"),
            ]
        };
        assert!(matches!(
            parse(&options("plan")),
            Ok(command) if matches!(command.action, DeployAction::Plan)
        ));
        let mut execute_arguments = options("execute");
        execute_arguments.push(OsString::from("--confirmation"));
        execute_arguments.push(OsString::from("phrase"));
        assert!(matches!(
            parse(&execute_arguments),
            Ok(command) if matches!(command.action, DeployAction::Execute)
        ));
        Ok(())
    }

    #[test]
    fn execute_without_a_confirmation_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::current_dir()?;
        let values = vec![
            "execute".to_owned(),
            "--repositories-dir".to_owned(),
            root.display().to_string(),
            "--config-dir".to_owned(),
            root.display().to_string(),
            "--repository-id".to_owned(),
            "repository-001".to_owned(),
            "--backup-id".to_owned(),
            "backup-001".to_owned(),
            "--profiles-dir".to_owned(),
            root.display().to_string(),
            "--target-profile-id".to_owned(),
            "profile-target".to_owned(),
            "--target-path".to_owned(),
            "/srv/app".to_owned(),
            "--json".to_owned(),
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
        assert_eq!(parse(&values).err(), Some(DeployFailure::usage()));
        Ok(())
    }

    #[test]
    fn deploying_to_an_unregistered_target_profile_fails_closed()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let repositories_dir = root.path().join("repositories");
        let profiles_dir = root.path().join("profiles");
        let config_dir = root.path().join("node");
        std::fs::create_dir_all(&repositories_dir)?;
        std::fs::create_dir_all(&profiles_dir)?;
        let secrets = MemoryStore::default();
        SigningIdentityManager::open(&config_dir)?.enroll_or_load(&secrets)?;
        let repository_path = root.path().join("repository");
        let repository_id = guardian_core::RepositoryId::parse("repository-001")?;
        drop(guardian_local_repository::LocalRepository::open(
            &repository_path,
            repository_id.clone(),
        )?);
        guardian_configuration::RepositoryStore::at(&repositories_dir).upsert(
            guardian_configuration::RepositoryRegistration::new(
                repository_id,
                "Test repository".to_owned(),
                std::fs::canonicalize(&repository_path)?,
            )
            .map_err(|_| std::io::Error::other("invalid registration"))?,
        )?;
        let command = super::DeployCommand {
            action: DeployAction::Plan,
            repositories_dir,
            config_dir,
            repository_id: "repository-001".to_owned(),
            backup_id: "backup-001".to_owned(),
            profiles_dir,
            target_profile_id: "profile-missing".to_owned(),
            target_path: guardian_core::RemoteTargetPath::parse("/srv/app")?,
            confirmation: None,
            vault_dir: None,
        };
        assert_eq!(
            execute(command, &secrets).err(),
            Some(DeployFailure::input())
        );
        Ok(())
    }

    #[derive(Default)]
    struct MemoryStore {
        values: std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
    }

    impl guardian_core::SecretStore for MemoryStore {
        fn load(
            &self,
            id: &guardian_core::CredentialId,
        ) -> Result<Option<guardian_core::SecretValue>, guardian_core::SecretStoreError> {
            let values = self
                .values
                .lock()
                .map_err(|_| guardian_core::SecretStoreError::OperationFailed)?;
            Ok(values
                .get(id.as_str())
                .cloned()
                .map(guardian_core::SecretValue::new))
        }

        fn store(
            &self,
            id: &guardian_core::CredentialId,
            secret: &guardian_core::SecretValue,
        ) -> Result<(), guardian_core::SecretStoreError> {
            let mut values = self
                .values
                .lock()
                .map_err(|_| guardian_core::SecretStoreError::OperationFailed)?;
            values.insert(id.as_str().to_owned(), secret.expose().to_vec());
            Ok(())
        }

        fn delete(
            &self,
            id: &guardian_core::CredentialId,
        ) -> Result<(), guardian_core::SecretStoreError> {
            let mut values = self
                .values
                .lock()
                .map_err(|_| guardian_core::SecretStoreError::OperationFailed)?;
            values.remove(id.as_str());
            Ok(())
        }
    }
}
