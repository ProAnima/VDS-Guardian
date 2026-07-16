#[path = "recovery/cli.rs"]
mod cli;

use cli::{args, path, run_guardian_cli};
use guardian_configuration::{RepositoryRegistration, RepositoryStore};
use guardian_core::{BackupId, RepositoryId, SecretStore};
use guardian_local_repository::LocalRepository;
use guardian_signing::{ManagedIdentity, SigningIdentityManager};
use serde_json::Value;
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};

pub struct RecoveryFixture {
    repository_id: RepositoryId,
    repository_path: PathBuf,
    bundle_path: PathBuf,
    passphrase_path: PathBuf,
}

pub fn initialize_and_export(
    root: &Path,
    repository: &LocalRepository,
    repository_id: RepositoryId,
    vault_dir: &Path,
    secrets: &dyn SecretStore,
) -> Result<(RecoveryFixture, ManagedIdentity), Box<dyn Error>> {
    let repositories_dir = root.join("original-repositories");
    fs::create_dir(&repositories_dir)?;
    register(&repositories_dir, repository, repository_id.clone())?;
    let signing_config_dir = root.join("original-signing");
    signing_enroll(&signing_config_dir, vault_dir)?;
    recovery_init(
        &repositories_dir,
        &repository_id,
        &signing_config_dir,
        vault_dir,
    )?;
    let passphrase_path = root.join("recovery-passphrase.txt");
    fs::write(&passphrase_path, b"clean-room recovery passphrase")?;
    let bundle_path = root.join("repository-recovery.json");
    recovery_export(
        &repositories_dir,
        &repository_id,
        vault_dir,
        &passphrase_path,
        &bundle_path,
    )?;
    let signer = SigningIdentityManager::open(signing_config_dir)?.load_ready(secrets)?;
    Ok((
        RecoveryFixture {
            repository_id,
            repository_path: repository.root().to_owned(),
            bundle_path,
            passphrase_path,
        },
        signer,
    ))
}

pub fn restore_on_clean_machine(
    root: &Path,
    fixture: &RecoveryFixture,
    backup_id: &BackupId,
    destination: &Path,
) -> Result<(), Box<dyn Error>> {
    let repositories_dir = root.join("clean-repositories");
    let vault_dir = root.join("clean-vault");
    let signing_config_dir = root.join("clean-signing");
    fs::create_dir(&repositories_dir)?;
    vault_init(&vault_dir)?;
    recovery_import(&repositories_dir, &vault_dir, fixture)?;
    let plan = restore_plan(
        &repositories_dir,
        &signing_config_dir,
        &vault_dir,
        &fixture.repository_id,
        backup_id,
        destination,
    )?;
    let confirmation = plan
        .pointer("/data/confirmation")
        .and_then(Value::as_str)
        .ok_or("restore plan omitted its confirmation phrase")?;
    restore_execute(
        &repositories_dir,
        &signing_config_dir,
        &vault_dir,
        &fixture.repository_id,
        backup_id,
        destination,
        confirmation,
    )?;
    Ok(())
}

fn register(
    repositories_dir: &Path,
    repository: &LocalRepository,
    repository_id: RepositoryId,
) -> Result<(), Box<dyn Error>> {
    let registration = RepositoryRegistration::new(
        repository_id,
        "Clean-room recovery source".to_owned(),
        repository.root().to_owned(),
    )?;
    RepositoryStore::at(repositories_dir).upsert(registration)?;
    Ok(())
}

fn signing_enroll(config: &Path, vault: &Path) -> Result<(), Box<dyn Error>> {
    run_guardian_cli(args(&[
        "signing",
        "enroll",
        "--config-dir",
        &path(config),
        "--vault-dir",
        &path(vault),
        "--json",
    ]))?;
    Ok(())
}

fn recovery_init(
    repositories: &Path,
    repository_id: &RepositoryId,
    signing: &Path,
    vault: &Path,
) -> Result<(), Box<dyn Error>> {
    run_guardian_cli(args(&[
        "recovery",
        "init",
        "--repositories-dir",
        &path(repositories),
        "--repository-id",
        repository_id.as_str(),
        "--signing-config-dir",
        &path(signing),
        "--vault-dir",
        &path(vault),
        "--json",
    ]))?;
    Ok(())
}

fn recovery_export(
    repositories: &Path,
    repository_id: &RepositoryId,
    vault: &Path,
    passphrase: &Path,
    bundle: &Path,
) -> Result<(), Box<dyn Error>> {
    let confirmation = format!("EXPORT RECOVERY BUNDLE FOR {}", repository_id.as_str());
    run_guardian_cli(args(&[
        "recovery",
        "export",
        "--repositories-dir",
        &path(repositories),
        "--repository-id",
        repository_id.as_str(),
        "--vault-dir",
        &path(vault),
        "--passphrase-file",
        &path(passphrase),
        "--output",
        &path(bundle),
        "--confirmation",
        &confirmation,
        "--json",
    ]))?;
    Ok(())
}

fn vault_init(vault: &Path) -> Result<(), Box<dyn Error>> {
    run_guardian_cli(args(&[
        "vault",
        "init",
        "--vault-dir",
        &path(vault),
        "--json",
    ]))?;
    Ok(())
}

fn recovery_import(
    repositories: &Path,
    vault: &Path,
    fixture: &RecoveryFixture,
) -> Result<(), Box<dyn Error>> {
    let confirmation = format!(
        "IMPORT RECOVERY BUNDLE FOR {}",
        fixture.repository_id.as_str()
    );
    run_guardian_cli(args(&[
        "recovery",
        "import",
        "--repositories-dir",
        &path(repositories),
        "--repository-id",
        fixture.repository_id.as_str(),
        "--repository-path",
        &path(&fixture.repository_path),
        "--vault-dir",
        &path(vault),
        "--passphrase-file",
        &path(&fixture.passphrase_path),
        "--input",
        &path(&fixture.bundle_path),
        "--confirmation",
        &confirmation,
        "--json",
    ]))?;
    Ok(())
}

fn restore_plan(
    repositories: &Path,
    signing: &Path,
    vault: &Path,
    repository_id: &RepositoryId,
    backup_id: &BackupId,
    destination: &Path,
) -> Result<Value, Box<dyn Error>> {
    run_guardian_cli(args(&restore_args(
        "plan",
        repositories,
        signing,
        vault,
        repository_id,
        backup_id,
        destination,
    )))
}

fn restore_execute(
    repositories: &Path,
    signing: &Path,
    vault: &Path,
    repository_id: &RepositoryId,
    backup_id: &BackupId,
    destination: &Path,
    confirmation: &str,
) -> Result<Value, Box<dyn Error>> {
    let mut values = restore_args(
        "execute",
        repositories,
        signing,
        vault,
        repository_id,
        backup_id,
        destination,
    );
    values.extend(["--confirmation".to_owned(), confirmation.to_owned()]);
    run_guardian_cli(args(&values))
}

fn restore_args(
    action: &str,
    repositories: &Path,
    signing: &Path,
    vault: &Path,
    repository_id: &RepositoryId,
    backup_id: &BackupId,
    destination: &Path,
) -> Vec<String> {
    vec![
        "restore".into(),
        action.into(),
        "--repositories-dir".into(),
        path(repositories),
        "--config-dir".into(),
        path(signing),
        "--repository-id".into(),
        repository_id.as_str().into(),
        "--backup-id".into(),
        backup_id.as_str().into(),
        "--destination".into(),
        path(destination),
        "--vault-dir".into(),
        path(vault),
        "--json".into(),
    ]
}
