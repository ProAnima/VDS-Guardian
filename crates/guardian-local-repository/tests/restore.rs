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
    local_repository.configure_recovery_key(&secrets)?;
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
    local_repository.configure_recovery_key(&secrets)?;
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
fn restore_reconstructs_an_encrypted_database_payload_alongside_the_filesystem_payload()
-> TestResult {
    let root = TestRoot::new()?;
    let local_repository = repository(&root)?;
    let signer = TestSigner::new();
    let secrets = MemorySecrets::default();
    local_repository.configure_recovery_key(&secrets)?;
    let backup_id = BackupId::parse("backup-with-database")?;
    let run = RunId::parse("run-with-database")?;
    let staging = local_repository.begin_staging(run.clone())?;
    let filesystem_path = PayloadPath::parse("payload/filesystem.tar.zst.enc")?;
    staging.write_payload(
        "filesystem",
        filesystem_path.clone(),
        "application/zstd",
        &archive()?,
    )?;
    let filesystem_payload = staging.encrypt_and_register_payload_file(
        "filesystem",
        filesystem_path,
        "application/zstd",
        &backup_id,
        &secrets,
    )?;
    let original_database = b"select 1;".repeat(32);
    let database_bytes = zstd::stream::encode_all(Cursor::new(&original_database[..]), 0)?;
    let database_path = PayloadPath::parse("payload/database.sqlite.zst.enc")?;
    staging.write_payload(
        "database",
        database_path.clone(),
        "application/vnd.sqlite3+zstd",
        &database_bytes,
    )?;
    let database_payload = staging.encrypt_and_register_payload_file(
        "database",
        database_path,
        "application/vnd.sqlite3+zstd",
        &backup_id,
        &secrets,
    )?;
    let mut manifest = manifest("backup-with-database", run)?;
    manifest.add_payload(filesystem_payload)?;
    manifest.add_payload(database_payload)?;
    staging.seal(manifest, timestamp("2026-07-15T09:00:00Z")?, &signer)?;
    let destination = root.path().join("database-target");
    let plan = local_repository.plan_restore(&backup_id, &destination, &signer)?;
    assert!(plan.database_payload.is_some());
    local_repository.execute_restore(
        &backup_id,
        &destination,
        &plan.confirmation,
        &signer,
        &secrets,
    )?;
    assert_eq!(std::fs::read(destination.join("srv/app/config"))?, b"safe");
    assert_eq!(
        std::fs::read(destination.join("database.sqlite"))?,
        original_database
    );
    Ok(())
}

#[test]
fn a_failed_second_payload_leaves_no_partial_destination() -> TestResult {
    let root = TestRoot::new()?;
    let local_repository = repository(&root)?;
    let signer = TestSigner::new();
    let secrets = MemorySecrets::default();
    local_repository.configure_recovery_key(&secrets)?;
    let backup_id = BackupId::parse("backup-partial-second-payload")?;
    let run = RunId::parse("run-partial-second-payload")?;
    let staging = local_repository.begin_staging(run.clone())?;
    let filesystem_path = PayloadPath::parse("payload/filesystem.tar.zst.enc")?;
    staging.write_payload(
        "filesystem",
        filesystem_path.clone(),
        "application/zstd",
        &archive()?,
    )?;
    let filesystem_payload = staging.encrypt_and_register_payload_file(
        "filesystem",
        filesystem_path,
        "application/zstd",
        &backup_id,
        &secrets,
    )?;
    let database_bytes =
        zstd::stream::encode_all(Cursor::new(b"select 1;".repeat(32).as_slice()), 0)?;
    let database_path = PayloadPath::parse("payload/database.sqlite.zst.enc")?;
    staging.write_payload(
        "database",
        database_path.clone(),
        "application/vnd.sqlite3+zstd",
        &database_bytes,
    )?;
    let database_payload = staging.encrypt_and_register_payload_file(
        "database",
        database_path,
        "application/vnd.sqlite3+zstd",
        &backup_id,
        &secrets,
    )?;
    // Only the database payload's key goes missing -- the filesystem
    // payload's own key stays available, so the filesystem extraction
    // succeeds before the database extraction fails.
    let missing_credential_id = database_payload
        .encryption
        .clone()
        .ok_or("database payload is encrypted")?
        .credential_id;
    let mut manifest = manifest("backup-partial-second-payload", run)?;
    manifest.add_payload(filesystem_payload)?;
    manifest.add_payload(database_payload)?;
    staging.seal(manifest, timestamp("2026-07-15T09:00:00Z")?, &signer)?;
    let destination = root.path().join("partial-second-payload-target");
    let plan = local_repository.plan_restore(&backup_id, &destination, &signer)?;
    assert!(plan.database_payload.is_some());
    // The recovery fallback (ADR 0013) must also be denied here, or it would
    // silently recover the "missing" key and defeat the point of this test:
    // proving a payload whose key is genuinely unavailable through any
    // channel fails closed without a partial destination.
    let recovery_credential_id = local_repository
        .recovery_credential_id()?
        .ok_or("recovery key should be configured")?;
    let secrets_missing_database_key = MissingOneKey {
        inner: &secrets,
        missing: missing_credential_id,
        recovery: recovery_credential_id,
    };
    assert!(matches!(
        local_repository.execute_restore(
            &backup_id,
            &destination,
            &plan.confirmation,
            &signer,
            &secrets_missing_database_key,
        ),
        Err(RepositoryError::Credential)
    ));
    // The whole point of this test: a failed *second* payload must not
    // leave the first payload's already-extracted tree behind at
    // `destination`, since that would block every future retry.
    assert!(!destination.exists());
    Ok(())
}

#[test]
fn encrypted_restore_fails_closed_when_the_key_is_missing() -> TestResult {
    let root = TestRoot::new()?;
    let local_repository = repository(&root)?;
    let signer = TestSigner::new();
    let secrets = MemorySecrets::default();
    local_repository.configure_recovery_key(&secrets)?;
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

#[test]
fn restore_falls_back_to_the_recovery_key_when_the_primary_key_is_missing() -> TestResult {
    let root = TestRoot::new()?;
    let local_repository = repository(&root)?;
    let signer = TestSigner::new();
    let secrets = MemorySecrets::default();
    local_repository.configure_recovery_key(&secrets)?;
    let run = RunId::parse("run-recovery-fallback")?;
    let staging = local_repository.begin_staging(run.clone())?;
    let path = PayloadPath::parse("payload/filesystem.tar.zst.enc")?;
    staging.write_payload("filesystem", path.clone(), "application/zstd", &archive()?)?;
    let payload = staging.encrypt_and_register_payload_file(
        "filesystem",
        path,
        "application/zstd",
        &BackupId::parse("backup-recovery-fallback")?,
        &secrets,
    )?;
    let primary_credential_id = payload
        .encryption
        .clone()
        .ok_or("payload is encrypted")?
        .credential_id;
    let mut manifest = manifest("backup-recovery-fallback", run)?;
    manifest.add_payload(payload)?;
    staging.seal(manifest, timestamp("2026-07-16T09:00:00Z")?, &signer)?;
    let destination = root.path().join("recovery-fallback-target");
    let plan = local_repository.plan_restore(
        &BackupId::parse("backup-recovery-fallback")?,
        &destination,
        &signer,
    )?;
    let secrets_missing_primary_key = MissingPrimaryKey {
        inner: &secrets,
        missing: primary_credential_id,
    };
    // The primary `SecretStore` entry is gone -- this must still succeed via
    // the manifest's own recovery-wrapped copy of the same key (ADR 0013).
    local_repository.execute_restore(
        &BackupId::parse("backup-recovery-fallback")?,
        &destination,
        &plan.confirmation,
        &signer,
        &secrets_missing_primary_key,
    )?;
    assert_eq!(std::fs::read(destination.join("srv/app/config"))?, b"safe");
    Ok(())
}

#[test]
fn restore_fails_closed_when_the_wrong_recovery_key_is_present() -> TestResult {
    let root = TestRoot::new()?;
    let local_repository = repository(&root)?;
    let signer = TestSigner::new();
    let secrets = MemorySecrets::default();
    local_repository.configure_recovery_key(&secrets)?;
    let recovery_credential_id = local_repository
        .recovery_credential_id()?
        .ok_or("recovery key should be configured")?;
    let run = RunId::parse("run-recovery-wrong-key")?;
    let staging = local_repository.begin_staging(run.clone())?;
    let path = PayloadPath::parse("payload/filesystem.tar.zst.enc")?;
    staging.write_payload("filesystem", path.clone(), "application/zstd", &archive()?)?;
    let payload = staging.encrypt_and_register_payload_file(
        "filesystem",
        path,
        "application/zstd",
        &BackupId::parse("backup-recovery-wrong-key")?,
        &secrets,
    )?;
    let primary_credential_id = payload
        .encryption
        .clone()
        .ok_or("payload is encrypted")?
        .credential_id;
    let mut manifest = manifest("backup-recovery-wrong-key", run)?;
    manifest.add_payload(payload)?;
    staging.seal(manifest, timestamp("2026-07-16T09:30:00Z")?, &signer)?;
    // Simulates an operator installing the wrong recovery bundle: the
    // recovery credential id is unchanged, but the bytes behind it are not
    // the key this payload was actually wrapped under.
    secrets.store(&recovery_credential_id, &SecretValue::new(vec![0_u8; 32]))?;
    let destination = root.path().join("recovery-wrong-key-target");
    let plan = local_repository.plan_restore(
        &BackupId::parse("backup-recovery-wrong-key")?,
        &destination,
        &signer,
    )?;
    let secrets_missing_primary_key = MissingPrimaryKey {
        inner: &secrets,
        missing: primary_credential_id,
    };
    assert!(matches!(
        local_repository.execute_restore(
            &BackupId::parse("backup-recovery-wrong-key")?,
            &destination,
            &plan.confirmation,
            &signer,
            &secrets_missing_primary_key,
        ),
        Err(RepositoryError::Credential)
    ));
    assert!(!destination.exists());
    Ok(())
}

/// Delegates to `inner` for every credential except `missing` -- simulates
/// only the primary payload key being gone while the repository's recovery
/// key remains reachable, proving the ADR 0013 fallback works (or, when the
/// stored recovery key itself is wrong, that it still fails closed).
struct MissingPrimaryKey<'a> {
    inner: &'a MemorySecrets,
    missing: CredentialId,
}

impl SecretStore for MissingPrimaryKey<'_> {
    fn load(&self, id: &CredentialId) -> Result<Option<SecretValue>, SecretStoreError> {
        if *id == self.missing {
            return Ok(None);
        }
        self.inner.load(id)
    }

    fn store(&self, id: &CredentialId, secret: &SecretValue) -> Result<(), SecretStoreError> {
        self.inner.store(id, secret)
    }

    fn delete(&self, id: &CredentialId) -> Result<(), SecretStoreError> {
        self.inner.delete(id)
    }
}

/// Delegates to `inner` for every credential except `missing` (the target
/// payload's own primary key) and `recovery` (the repository's recovery
/// credential) — both must be unavailable to simulate a payload's key being
/// truly gone now that a recovery-key fallback exists (ADR 0013), while
/// every other payload's own primary key stays reachable.
struct MissingOneKey<'a> {
    inner: &'a MemorySecrets,
    missing: CredentialId,
    recovery: CredentialId,
}

impl SecretStore for MissingOneKey<'_> {
    fn load(&self, id: &CredentialId) -> Result<Option<SecretValue>, SecretStoreError> {
        if *id == self.missing || *id == self.recovery {
            return Ok(None);
        }
        self.inner.load(id)
    }

    fn store(&self, id: &CredentialId, secret: &SecretValue) -> Result<(), SecretStoreError> {
        self.inner.store(id, secret)
    }

    fn delete(&self, id: &CredentialId) -> Result<(), SecretStoreError> {
        self.inner.delete(id)
    }
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
