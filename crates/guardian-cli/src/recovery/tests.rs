use super::{RecoveryAction, RecoveryCommand, RecoveryFailure, RecoveryOutput, execute, parse};
use guardian_core::{
    BackupId, CredentialId, PayloadPath, PlanId, PlanReference, Producer, ProfileId, RepositoryId,
    RunId, SecretStore, SecretStoreError, SecretValue, SourceIdentity, Timestamp,
};
use guardian_local_repository::{
    LocalRepository, export_confirmation_phrase, import_confirmation_phrase,
};
use guardian_signing::SigningIdentityManager;
use std::{collections::HashMap, ffi::OsString, fs, io::Read, path::PathBuf, sync::Mutex};

#[test]
fn exact_recovery_actions_are_distinct() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?;
    let options = |action: &str| {
        let mut values = vec![
            OsString::from(action),
            OsString::from("--repositories-dir"),
            root.as_os_str().to_owned(),
            OsString::from("--repository-id"),
            OsString::from("repository-001"),
        ];
        if action == "init" {
            values.extend([
                OsString::from("--signing-config-dir"),
                root.as_os_str().to_owned(),
            ]);
        }
        values.push(OsString::from("--json"));
        values
    };
    assert!(matches!(
        parse(&options("init")),
        Ok(command) if matches!(command.action, RecoveryAction::Init)
    ));
    assert!(matches!(
        parse(&options("status")),
        Ok(command) if matches!(command.action, RecoveryAction::Status)
    ));
    Ok(())
}

#[test]
fn export_rejects_a_mismatched_confirmation_phrase() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let (repositories_dir, repository_id) = registered_repository(&root, "repository-mismatch")?;
    let secrets = MemoryStore::default();
    let config_dir = root.path().join("node");
    SigningIdentityManager::open(&config_dir)?.enroll_or_load(&secrets)?;
    let mut init_command =
        recovery_command(RecoveryAction::Init, &repositories_dir, &repository_id);
    init_command.signing_config_dir = Some(config_dir);
    execute(init_command, &secrets).map_err(|_| std::io::Error::other("init failed"))?;
    let passphrase_file = root.path().join("passphrase.txt");
    fs::write(&passphrase_file, "correct horse battery staple")?;
    let mut command = recovery_command(RecoveryAction::Export, &repositories_dir, &repository_id);
    command.passphrase_file = Some(passphrase_file);
    command.output = Some(root.path().join("bundle.json"));
    command.confirmation = Some("WRONG PHRASE".to_owned());
    assert_eq!(
        execute(command, &secrets).err(),
        Some(RecoveryFailure::confirmation_mismatch())
    );
    Ok(())
}

#[test]
fn export_fails_closed_when_recovery_was_never_initialized()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let (repositories_dir, repository_id) = registered_repository(&root, "repository-uninit")?;
    let secrets = MemoryStore::default();
    let passphrase_file = root.path().join("passphrase.txt");
    fs::write(&passphrase_file, "correct horse battery staple")?;
    let mut command = recovery_command(RecoveryAction::Export, &repositories_dir, &repository_id);
    command.passphrase_file = Some(passphrase_file);
    command.output = Some(root.path().join("bundle.json"));
    command.confirmation = Some(export_confirmation_phrase(&RepositoryId::parse(
        repository_id.clone(),
    )?));
    assert_eq!(
        execute(command, &secrets).err(),
        Some(RecoveryFailure::not_configured())
    );
    Ok(())
}

#[test]
fn init_export_import_recovers_byte_identical_key_material_on_a_fresh_secret_store()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let (repositories_dir, repository_id) =
        registered_repository(&root, "repository-recovery-cli")?;
    let parsed_repository_id = RepositoryId::parse(repository_id.clone())?;
    let original_secrets = MemoryStore::default();
    let config_dir = root.path().join("node");
    let identity = SigningIdentityManager::open(&config_dir)?.enroll_or_load(&original_secrets)?;

    let mut init_command =
        recovery_command(RecoveryAction::Init, &repositories_dir, &repository_id);
    init_command.signing_config_dir = Some(config_dir);
    let init = execute(init_command, &original_secrets)
        .map_err(|_| std::io::Error::other("recovery init failed"))?;
    let init_credential_id = match init {
        RecoveryOutput::Init { credential_id } => credential_id,
        _ => return Err("expected an init output".into()),
    };

    // A real encrypted payload, captured directly against the same
    // repository -- proves the exported/imported key is the one this
    // specific backup was actually wrapped under, not just any key.
    let registration = guardian_configuration::RepositoryStore::at(&repositories_dir)
        .get(&parsed_repository_id)?
        .ok_or("repository should be registered")?;
    let repository = LocalRepository::open(&registration.path, parsed_repository_id.clone())?;
    let backup_id = BackupId::parse("backup-recovery-cli")?;
    let run_id = RunId::parse("run-recovery-cli")?;
    let staging = repository.begin_staging(run_id.clone())?;
    let path = PayloadPath::parse("payload/filesystem.tar.zst.enc")?;
    staging.write_payload(
        "filesystem",
        path.clone(),
        "application/zstd",
        b"payload bytes",
    )?;
    let payload = staging.encrypt_and_register_payload_file(
        "filesystem",
        path,
        "application/zstd",
        &backup_id,
        &original_secrets,
    )?;
    let mut manifest = guardian_core::Manifest::new(
        backup_id,
        run_id,
        Timestamp::parse("2026-07-16T10:00:00Z")?,
        Producer {
            name: "guardian-cli test".to_owned(),
            version: "0.1.0".to_owned(),
            platform: "test".to_owned(),
        },
        SourceIdentity {
            profile_id: ProfileId::parse("profile-recovery-cli")?,
            host_key_fingerprint: "SHA256:fixture".to_owned(),
        },
        PlanReference {
            plan_id: PlanId::parse("plan-recovery-cli")?,
            version: 1,
            sha256: "a".repeat(64),
        },
    );
    manifest.add_payload(payload)?;
    staging.seal(
        manifest,
        Timestamp::parse("2026-07-16T10:05:00Z")?,
        &identity,
    )?;

    let passphrase_file = root.path().join("passphrase.txt");
    fs::write(&passphrase_file, "correct horse battery staple")?;
    let output_path = root.path().join("bundle.json");
    let mut export_command =
        recovery_command(RecoveryAction::Export, &repositories_dir, &repository_id);
    export_command.passphrase_file = Some(passphrase_file.clone());
    export_command.output = Some(output_path.clone());
    export_command.confirmation = Some(export_confirmation_phrase(&parsed_repository_id));
    let export = execute(export_command, &original_secrets)
        .map_err(|_| std::io::Error::other("recovery export failed"))?;
    assert!(matches!(export, RecoveryOutput::Export { .. }));

    // Import into a fresh, empty `SecretStore` -- simulating a clean
    // machine that has the repository directory and the recovery
    // bundle, but no original OS credential-store state.
    let clean_machine_secrets = MemoryStore::default();
    let clean_repositories_dir = root.path().join("clean-repositories");
    let mut import_command = recovery_command(
        RecoveryAction::Import,
        &clean_repositories_dir,
        &repository_id,
    );
    import_command.repository_path = Some(registration.path.clone());
    import_command.passphrase_file = Some(passphrase_file);
    import_command.input = Some(output_path);
    import_command.confirmation = Some(import_confirmation_phrase(&parsed_repository_id));
    let import = execute(import_command, &clean_machine_secrets)
        .map_err(|_| std::io::Error::other("recovery import failed"))?;
    let imported_credential_id = match import {
        RecoveryOutput::Import { credential_id } => credential_id,
        _ => return Err("expected an import output".into()),
    };
    // Reuses the id `init` already recorded -- it does not mint a
    // second one.
    assert_eq!(imported_credential_id, init_credential_id);

    let original_key = repository
        .export_recovery_key(&original_secrets)?
        .ok_or("recovery key should be configured")?;
    let recovered_key = repository
        .export_recovery_key(&clean_machine_secrets)?
        .ok_or("recovery key should be importable")?;
    assert_eq!(original_key.expose(), recovered_key.expose());

    let (mut plaintext, expected_bytes) = repository.open_deploy_payload_reader(
        &BackupId::parse("backup-recovery-cli")?,
        &PayloadPath::parse("payload/filesystem.tar.zst.enc")?,
        &identity,
        &clean_machine_secrets,
    )?;
    let mut restored = Vec::new();
    plaintext.read_to_end(&mut restored)?;
    assert_eq!(expected_bytes, 13);
    assert_eq!(restored, b"payload bytes");
    Ok(())
}

#[test]
fn import_with_wrong_passphrase_leaves_no_repository_registration()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let (repositories_dir, repository_id) =
        registered_repository(&root, "repository-wrong-passphrase")?;
    let parsed_repository_id = RepositoryId::parse(repository_id.clone())?;
    let original_secrets = MemoryStore::default();
    let config_dir = root.path().join("node");
    SigningIdentityManager::open(&config_dir)?.enroll_or_load(&original_secrets)?;

    let mut init_command =
        recovery_command(RecoveryAction::Init, &repositories_dir, &repository_id);
    init_command.signing_config_dir = Some(config_dir);
    execute(init_command, &original_secrets)
        .map_err(|_| std::io::Error::other("recovery init failed"))?;

    let correct_passphrase = root.path().join("correct-passphrase.txt");
    fs::write(&correct_passphrase, "correct horse battery staple")?;
    let bundle_path = root.path().join("bundle.json");
    let mut export_command =
        recovery_command(RecoveryAction::Export, &repositories_dir, &repository_id);
    export_command.passphrase_file = Some(correct_passphrase);
    export_command.output = Some(bundle_path.clone());
    export_command.confirmation = Some(export_confirmation_phrase(&parsed_repository_id));
    execute(export_command, &original_secrets)
        .map_err(|_| std::io::Error::other("recovery export failed"))?;

    let registration = guardian_configuration::RepositoryStore::at(&repositories_dir)
        .get(&parsed_repository_id)?
        .ok_or("repository should be registered")?;
    let wrong_passphrase = root.path().join("wrong-passphrase.txt");
    fs::write(&wrong_passphrase, "this is not the passphrase")?;
    let clean_repositories_dir = root.path().join("clean-repositories");
    let clean_machine_secrets = MemoryStore::default();
    let mut import_command = recovery_command(
        RecoveryAction::Import,
        &clean_repositories_dir,
        &repository_id,
    );
    import_command.repository_path = Some(registration.path);
    import_command.passphrase_file = Some(wrong_passphrase);
    import_command.input = Some(bundle_path);
    import_command.confirmation = Some(import_confirmation_phrase(&parsed_repository_id));

    assert_eq!(
        execute(import_command, &clean_machine_secrets).err(),
        Some(RecoveryFailure::bundle_operation())
    );
    assert!(
        guardian_configuration::RepositoryStore::at(&clean_repositories_dir)
            .get(&parsed_repository_id)?
            .is_none()
    );
    assert!(
        clean_machine_secrets
            .values
            .lock()
            .map_err(|_| "secret-store lock was poisoned")?
            .is_empty()
    );
    Ok(())
}

fn recovery_command(
    action: RecoveryAction,
    repositories_dir: &std::path::Path,
    repository_id: &str,
) -> RecoveryCommand {
    RecoveryCommand {
        action,
        repositories_dir: repositories_dir.to_path_buf(),
        repository_id: repository_id.to_owned(),
        passphrase_file: None,
        output: None,
        input: None,
        confirmation: None,
        vault_dir: None,
        signing_config_dir: None,
        repository_path: None,
    }
}

/// Opens and registers a fresh `LocalRepository`, mirroring
/// `restore.rs`'s own established test pattern for this exact setup.
fn registered_repository(
    root: &tempfile::TempDir,
    repository_id: &str,
) -> Result<(PathBuf, String), Box<dyn std::error::Error>> {
    let repositories_dir = root.path().join("repositories");
    let repository_path = root.path().join(format!("{repository_id}-data"));
    let parsed_id = RepositoryId::parse(repository_id)?;
    let repository = LocalRepository::open(&repository_path, parsed_id.clone())?;
    drop(repository);
    fs::create_dir_all(&repositories_dir)?;
    guardian_configuration::RepositoryStore::at(&repositories_dir).upsert(
        guardian_configuration::RepositoryRegistration::new(
            parsed_id,
            "Test repository".to_owned(),
            fs::canonicalize(&repository_path)?,
        )
        .map_err(|_| std::io::Error::other("invalid registration"))?,
    )?;
    Ok((repositories_dir, repository_id.to_owned()))
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
