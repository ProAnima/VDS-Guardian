mod support;

use guardian_archive::TarZstdWriter;
use guardian_core::{ArchivePath, BackupId, PayloadPath, RunId};
use guardian_core::{CredentialId, SecretStore, SecretStoreError, SecretValue};
use guardian_local_repository::RepositoryError;
use std::{collections::HashMap, io::Cursor, sync::Mutex};
use support::{TestResult, TestRoot, TestSigner, manifest, repository, timestamp};

#[test]
fn restore_plan_rechecks_the_sealed_backup_and_requires_a_new_target() -> TestResult {
    let root = TestRoot::new()?;
    let local_repository = repository(&root)?;
    let signer = TestSigner::new();
    let run = RunId::parse("run-restore")?;
    let staging = local_repository.begin_staging(run.clone())?;
    let payload = staging.write_payload(
        "filesystem",
        PayloadPath::parse("payload/filesystem.tar.zst")?,
        "application/zstd",
        b"payload",
    )?;
    let mut manifest = manifest("backup-restore", run)?;
    manifest.add_payload(payload)?;
    staging.seal(manifest, timestamp("2026-07-14T20:00:00Z")?, &signer)?;
    let destination = root.path().join("new-target");
    let plan = local_repository.plan_restore(
        &BackupId::parse("backup-restore")?,
        &destination,
        &signer,
    )?;
    assert_eq!(plan.backup_id.as_str(), "backup-restore");
    std::fs::create_dir(&destination)?;
    assert!(matches!(
        local_repository.plan_restore(&BackupId::parse("backup-restore")?, &destination, &signer),
        Err(RepositoryError::RestoreDestinationExists)
    ));
    Ok(())
}

#[test]
fn approved_restore_extracts_a_verified_payload_to_a_new_target() -> TestResult {
    let root = TestRoot::new()?;
    let local_repository = repository(&root)?;
    let signer = TestSigner::new();
    let run = RunId::parse("run-extract")?;
    let staging = local_repository.begin_staging(run.clone())?;
    let payload = staging.write_payload(
        "filesystem",
        PayloadPath::parse("payload/filesystem.tar.zst")?,
        "application/zstd",
        &archive()?,
    )?;
    let mut manifest = manifest("backup-extract", run)?;
    manifest.add_payload(payload)?;
    staging.seal(manifest, timestamp("2026-07-14T20:00:00Z")?, &signer)?;
    drop(local_repository);
    let repository = repository(&root)?;
    let destination = root.path().join("new-target");
    let plan =
        repository.plan_restore(&BackupId::parse("backup-extract")?, &destination, &signer)?;
    repository.execute_restore(
        &BackupId::parse("backup-extract")?,
        &destination,
        &plan.confirmation,
        &signer,
        &NoopSecrets,
    )?;
    assert_eq!(std::fs::read(destination.join("srv/app/config"))?, b"safe");
    Ok(())
}

#[test]
fn encrypted_payload_restores_only_with_its_keyring_key() -> TestResult {
    let root = TestRoot::new()?;
    let local_repository = repository(&root)?;
    let signer = TestSigner::new();
    let secrets = MemorySecrets::default();
    let run = RunId::parse("run-encrypted-extract")?;
    let staging = local_repository.begin_staging(run.clone())?;
    let path = PayloadPath::parse("payload/filesystem.tar.zst.enc")?;
    staging.write_payload("filesystem", path.clone(), "application/zstd", &archive()?)?;
    let payload = staging.encrypt_and_register_payload_file(
        "filesystem",
        path,
        "application/zstd",
        &BackupId::parse("backup-encrypted-extract")?,
        &secrets,
    )?;
    assert!(payload.encryption.is_some());
    let mut manifest = manifest("backup-encrypted-extract", run)?;
    manifest.add_payload(payload)?;
    staging.seal(manifest, timestamp("2026-07-14T20:00:00Z")?, &signer)?;
    let destination = root.path().join("encrypted-target");
    let plan = local_repository.plan_restore(
        &BackupId::parse("backup-encrypted-extract")?,
        &destination,
        &signer,
    )?;
    local_repository.execute_restore(
        &BackupId::parse("backup-encrypted-extract")?,
        &destination,
        &plan.confirmation,
        &signer,
        &secrets,
    )?;
    assert_eq!(std::fs::read(destination.join("srv/app/config"))?, b"safe");
    Ok(())
}

#[test]
fn discarding_an_encrypted_staging_run_revokes_its_payload_key() -> TestResult {
    let root = TestRoot::new()?;
    let local_repository = repository(&root)?;
    let secrets = MemorySecrets::default();
    let run = RunId::parse("run-discard-encrypted")?;
    let staging = local_repository.begin_staging(run.clone())?;
    let path = PayloadPath::parse("payload/filesystem.tar.zst.enc")?;
    staging.write_payload("filesystem", path.clone(), "application/zstd", &archive()?)?;
    let payload = staging.encrypt_and_register_payload_file(
        "filesystem",
        path,
        "application/zstd",
        &BackupId::parse("backup-discard-encrypted")?,
        &secrets,
    )?;
    let credential_id = payload
        .encryption
        .ok_or("payload is encrypted")?
        .credential_id;
    assert!(secrets.load(&credential_id)?.is_some());
    staging.discard()?;
    assert!(secrets.load(&credential_id)?.is_none());
    Ok(())
}

#[test]
fn encrypted_restore_fails_closed_when_the_key_is_missing() -> TestResult {
    let root = TestRoot::new()?;
    let local_repository = repository(&root)?;
    let signer = TestSigner::new();
    let secrets = MemorySecrets::default();
    let run = RunId::parse("run-missing-key")?;
    let staging = local_repository.begin_staging(run.clone())?;
    let path = PayloadPath::parse("payload/filesystem.tar.zst.enc")?;
    staging.write_payload("filesystem", path.clone(), "application/zstd", &archive()?)?;
    let payload = staging.encrypt_and_register_payload_file(
        "filesystem",
        path,
        "application/zstd",
        &BackupId::parse("backup-missing-key")?,
        &secrets,
    )?;
    let mut manifest = manifest("backup-missing-key", run)?;
    manifest.add_payload(payload)?;
    staging.seal(manifest, timestamp("2026-07-14T20:00:00Z")?, &signer)?;
    let destination = root.path().join("missing-key-target");
    let plan = local_repository.plan_restore(
        &BackupId::parse("backup-missing-key")?,
        &destination,
        &signer,
    )?;
    assert!(matches!(
        local_repository.execute_restore(
            &BackupId::parse("backup-missing-key")?,
            &destination,
            &plan.confirmation,
            &signer,
            &NoopSecrets,
        ),
        Err(RepositoryError::Credential)
    ));
    assert!(!destination.exists());
    Ok(())
}

struct NoopSecrets;

impl SecretStore for NoopSecrets {
    fn load(&self, _: &CredentialId) -> Result<Option<SecretValue>, SecretStoreError> {
        Ok(None)
    }

    fn store(&self, _: &CredentialId, _: &SecretValue) -> Result<(), SecretStoreError> {
        Ok(())
    }

    fn delete(&self, _: &CredentialId) -> Result<(), SecretStoreError> {
        Ok(())
    }
}

#[derive(Default)]
struct MemorySecrets(Mutex<HashMap<String, Vec<u8>>>);

impl SecretStore for MemorySecrets {
    fn load(&self, id: &CredentialId) -> Result<Option<SecretValue>, SecretStoreError> {
        let values = self
            .0
            .lock()
            .map_err(|_| SecretStoreError::OperationFailed)?;
        Ok(values.get(id.as_str()).cloned().map(SecretValue::new))
    }

    fn store(&self, id: &CredentialId, secret: &SecretValue) -> Result<(), SecretStoreError> {
        let mut values = self
            .0
            .lock()
            .map_err(|_| SecretStoreError::OperationFailed)?;
        values.insert(id.as_str().to_owned(), secret.expose().to_vec());
        Ok(())
    }

    fn delete(&self, id: &CredentialId) -> Result<(), SecretStoreError> {
        let mut values = self
            .0
            .lock()
            .map_err(|_| SecretStoreError::OperationFailed)?;
        values.remove(id.as_str());
        Ok(())
    }
}

fn archive() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let directory = ArchivePath::parse("srv")?;
    let app = ArchivePath::parse("srv/app")?;
    let file = ArchivePath::parse("srv/app/config")?;
    let mut contents = Cursor::new(b"safe".as_slice());
    let mut writer = TarZstdWriter::new(Vec::new())?;
    writer.append_directory(&directory)?;
    writer.append_directory(&app)?;
    writer.append_file(&file, 4, &mut contents)?;
    Ok(writer.finish()?)
}
