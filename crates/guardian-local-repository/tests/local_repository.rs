mod support;

use guardian_core::{
    BackupId, CredentialId, Manifest, ManifestError, PayloadPath, RunId, SecretStore,
    SecretStoreError, SecretValue, SigningError, VerificationState,
};
use guardian_encryption::PayloadKey;
use guardian_local_repository::RepositoryError;
use std::collections::HashMap;
use std::fs;
use std::sync::Mutex;
use std::time::Duration;
use support::{
    RejectingSigner, TestResult, TestRoot, TestSigner, manifest, repository, timestamp,
    verify_stored_signature,
};

#[test]
fn simulated_bytes_become_a_signed_independent_backup() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let signer = TestSigner::new();
    let run_id = RunId::parse("run-001")?;
    let staging = repository.begin_staging(run_id.clone())?;
    let payload = staging.write_payload(
        "filesystem",
        PayloadPath::parse("payload/filesystem-000.tar.zst")?,
        "application/zstd",
        b"independent backup bytes",
    )?;
    let mut first_manifest = manifest("backup-001", run_id)?;
    first_manifest.add_payload(payload)?;
    let sealed = staging.seal(first_manifest, timestamp("2026-07-13T12:05:00Z")?, &signer)?;

    assert!(sealed.path.join("manifest.json").is_file());
    assert!(sealed.path.join("manifest.sig").is_file());
    assert!(sealed.path.join("reports/verification.json").is_file());
    verify_stored_signature(&sealed.path, &signer)?;
    let stored: Manifest = serde_json::from_slice(&fs::read(sealed.path.join("manifest.json"))?)?;
    assert_eq!(stored.verification_state, VerificationState::Verified);
    assert!(!repository.root().join("staging/run-001").exists());

    let second_run = RunId::parse("run-002")?;
    let second_stage = repository.begin_staging(second_run.clone())?;
    let second_payload = second_stage.write_payload(
        "filesystem",
        PayloadPath::parse("payload/filesystem-000.tar.zst")?,
        "application/zstd",
        b"different independent bytes",
    )?;
    let mut second_manifest = manifest("backup-002", second_run)?;
    second_manifest.add_payload(second_payload)?;
    let second = second_stage.seal(second_manifest, timestamp("2026-07-13T12:06:00Z")?, &signer)?;
    assert_ne!(sealed.path, second.path);
    assert_eq!(
        fs::read(sealed.path.join("payload/filesystem-000.tar.zst"))?,
        b"independent backup bytes"
    );
    assert_eq!(
        fs::read(second.path.join("payload/filesystem-000.tar.zst"))?,
        b"different independent bytes"
    );
    Ok(())
}

#[test]
fn reserved_payload_is_registered_from_staging_without_memory_buffering() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let staging = repository.begin_staging(RunId::parse("run-stream")?)?;
    let path = PayloadPath::parse("payload/filesystem-000.tar.zst")?;
    let destination = staging.reserve_payload_destination(&path)?;
    fs::write(&destination, b"streamed archive")?;
    let payload = staging.register_payload_file("filesystem", path.clone(), "application/zstd")?;
    assert_eq!(payload.path, path);
    assert_eq!(payload.byte_length, 16);
    assert!(staging.reserve_payload_destination(&path).is_err());
    Ok(())
}

#[test]
fn corruption_is_quarantined_and_never_published() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let run_id = RunId::parse("run-corrupt")?;
    let staging = repository.begin_staging(run_id.clone())?;
    let path = PayloadPath::parse("payload/filesystem.tar.zst")?;
    let payload = staging.write_payload("filesystem", path, "application/zstd", b"clean")?;
    fs::write(
        repository
            .root()
            .join("staging/run-corrupt/payload/filesystem.tar.zst"),
        b"infected",
    )?;
    let mut manifest = manifest("backup-corrupt", run_id)?;
    manifest.add_payload(payload)?;
    let result = staging.seal(
        manifest,
        timestamp("2026-07-13T12:05:00Z")?,
        &TestSigner::new(),
    );

    assert!(matches!(result, Err(RepositoryError::IntegrityFailure)));
    assert!(!repository.root().join("backups/backup-corrupt").exists());
    assert!(
        repository
            .root()
            .join("quarantine/run-corrupt/quarantine.json")
            .is_file()
    );
    Ok(())
}

#[test]
fn repository_lock_rejects_a_concurrent_writer() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let first = repository.begin_staging(RunId::parse("run-one")?)?;
    let second = repository.begin_staging(RunId::parse("run-two")?);
    assert!(matches!(second, Err(RepositoryError::Busy)));
    drop(first);
    assert!(repository.begin_staging(RunId::parse("run-two")?).is_ok());
    Ok(())
}

#[test]
fn interrupted_staging_is_recovered_to_quarantine() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let staging = repository.begin_staging(RunId::parse("run-abandoned")?)?;
    drop(staging);
    assert_eq!(repository.recover_abandoned_staging(Duration::ZERO)?, 1);
    assert!(!repository.root().join("staging/run-abandoned").exists());
    assert!(repository.root().join("quarantine/run-abandoned").is_dir());
    Ok(())
}

#[test]
fn recover_abandoned_staging_ignores_the_restore_scratch_directory() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let staging = repository.begin_staging(RunId::parse("run-abandoned")?)?;
    drop(staging);
    assert_eq!(repository.recover_abandoned_staging(Duration::ZERO)?, 1);
    assert!(repository.root().join("staging/restore").is_dir());
    Ok(())
}

#[test]
fn list_sealed_backups_returns_only_verified_sealed_backups() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let signer = TestSigner::new();
    assert!(repository.list_sealed_backups(&signer)?.is_empty());
    let run_id = RunId::parse("run-list")?;
    let staging = repository.begin_staging(run_id.clone())?;
    let payload = staging.write_payload(
        "filesystem",
        PayloadPath::parse("payload/filesystem-000.tar.zst")?,
        "application/zstd",
        b"listed backup bytes",
    )?;
    let mut manifest = manifest("backup-list", run_id)?;
    manifest.add_payload(payload)?;
    staging.seal(manifest, timestamp("2026-07-13T12:05:00Z")?, &signer)?;
    let inventory = repository.list_sealed_backups(&signer)?;
    assert_eq!(inventory.len(), 1);
    assert_eq!(inventory[0].backup_id.as_str(), "backup-list");
    Ok(())
}

#[test]
fn recover_abandoned_restores_removes_only_stale_scratch_files() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let scratch = repository.root().join("staging/restore");
    fs::write(scratch.join("stale.tmp"), b"leftover plaintext")?;
    assert_eq!(repository.recover_abandoned_restores(Duration::ZERO)?, 1);
    assert!(!scratch.join("stale.tmp").exists());
    Ok(())
}

#[test]
fn payload_writes_cannot_escape_the_payload_directory() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let staging = repository.begin_staging(RunId::parse("run-path")?)?;
    let result = staging.write_payload(
        "filesystem",
        PayloadPath::parse("reports/forged.json")?,
        "application/json",
        b"forged",
    );
    assert!(matches!(
        result,
        Err(RepositoryError::UnsafeFilesystemEntry)
    ));
    Ok(())
}

#[test]
fn signature_failure_is_quarantined() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let run_id = RunId::parse("run-signature")?;
    let staging = repository.begin_staging(run_id.clone())?;
    let payload = staging.write_payload(
        "filesystem",
        PayloadPath::parse("payload/filesystem.tar.zst")?,
        "application/zstd",
        b"signed bytes",
    )?;
    let mut manifest = manifest("backup-signature", run_id)?;
    manifest.add_payload(payload)?;
    let result = staging.seal(
        manifest,
        timestamp("2026-07-13T12:05:00Z")?,
        &RejectingSigner(TestSigner::new()),
    );
    assert!(matches!(
        result,
        Err(RepositoryError::Signing(SigningError::VerificationFailed))
    ));
    assert!(!repository.root().join("backups/backup-signature").exists());
    assert!(repository.root().join("quarantine/run-signature").is_dir());
    Ok(())
}

#[test]
fn unlisted_payload_file_prevents_seal() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let run_id = RunId::parse("run-unlisted")?;
    let staging = repository.begin_staging(run_id.clone())?;
    let listed = staging.write_payload(
        "filesystem",
        PayloadPath::parse("payload/listed.tar.zst")?,
        "application/zstd",
        b"listed",
    )?;
    let _unlisted = staging.write_payload(
        "filesystem",
        PayloadPath::parse("payload/unlisted.tar.zst")?,
        "application/zstd",
        b"unlisted",
    )?;
    let mut manifest = manifest("backup-unlisted", run_id)?;
    manifest.add_payload(listed)?;
    let result = staging.seal(
        manifest,
        timestamp("2026-07-13T12:05:00Z")?,
        &TestSigner::new(),
    );
    assert!(matches!(result, Err(RepositoryError::IntegrityFailure)));
    assert!(!repository.root().join("backups/backup-unlisted").exists());
    assert!(repository.root().join("quarantine/run-unlisted").is_dir());
    Ok(())
}

#[test]
fn finalized_manifest_cannot_be_resealed() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let run_id = RunId::parse("run-reseal")?;
    let staging = repository.begin_staging(run_id.clone())?;
    let payload = staging.write_payload(
        "filesystem",
        PayloadPath::parse("payload/filesystem.tar.zst")?,
        "application/zstd",
        b"payload",
    )?;
    let mut finalized = manifest("backup-reseal", run_id)?;
    finalized.add_payload(payload)?;
    finalized.verification_state = VerificationState::Verified;
    let result = staging.seal(
        finalized,
        timestamp("2026-07-13T12:05:00Z")?,
        &TestSigner::new(),
    );
    assert!(matches!(
        result,
        Err(RepositoryError::Manifest(ManifestError::AlreadyFinalized))
    ));
    assert!(!repository.root().join("backups/backup-reseal").exists());
    assert!(repository.root().join("quarantine/run-reseal").is_dir());
    Ok(())
}

#[test]
fn configure_recovery_key_is_write_once() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let secrets = MemorySecrets::default();
    assert!(repository.recovery_credential_id()?.is_none());
    let first_id = repository.configure_recovery_key(&secrets)?;
    assert_eq!(repository.recovery_credential_id()?, Some(first_id));
    // Regenerating would silently orphan every payload already wrapped
    // under the first key -- must fail closed, not replace it.
    assert!(matches!(
        repository.configure_recovery_key(&secrets),
        Err(RepositoryError::RecoveryKeyAlreadyConfigured)
    ));
    Ok(())
}

#[test]
fn encrypted_capture_requires_a_configured_recovery_key() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let secrets = MemorySecrets::default();
    let staging = repository.begin_staging(RunId::parse("run-recovery-required")?)?;
    let path = PayloadPath::parse("payload/filesystem.tar.zst.enc")?;
    staging.write_payload("filesystem", path.clone(), "application/zstd", b"plaintext")?;
    // Without a configured recovery key, every new encrypted payload must
    // fail closed rather than seal a backup only the current OS keyring can
    // ever decrypt (ADR 0013). `configure_recovery_key` itself needs the
    // repository lock this still-live `staging` run already holds, so it is
    // deliberately not attempted here -- a fresh staging run after
    // `recovery init` is exercised separately below.
    assert!(matches!(
        staging.encrypt_and_register_payload_file(
            "filesystem",
            path,
            "application/zstd",
            &BackupId::parse("backup-recovery-required")?,
            &secrets,
        ),
        Err(RepositoryError::RecoveryKeyNotConfigured)
    ));
    Ok(())
}

#[test]
fn import_recovery_key_reuses_an_already_recorded_credential_id() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let original_secrets = MemorySecrets::default();
    let recorded_id = repository.configure_recovery_key(&original_secrets)?;

    // Simulates a genuine clean-machine restore: the same repository
    // directory (so `repository.json` already records `recorded_id`), but
    // a completely different, empty `SecretStore`. Import must reuse the
    // already-recorded id rather than failing closed as "already
    // configured" -- that error is for `configure_recovery_key`'s
    // never-regenerate rule, not for restoring an already-known key.
    let clean_machine_secrets = MemorySecrets::default();
    let imported_id =
        repository.import_recovery_key(&clean_machine_secrets, PayloadKey::generate())?;
    assert_eq!(imported_id, recorded_id);
    assert!(clean_machine_secrets.load(&imported_id)?.is_some());
    Ok(())
}

#[test]
fn import_recovery_key_never_overwrites_a_different_existing_secret() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let secrets = MemorySecrets::default();
    repository.configure_recovery_key(&secrets)?;
    let original = repository
        .export_recovery_key(&secrets)?
        .ok_or("recovery key should exist")?;

    repository.import_recovery_key(&secrets, PayloadKey::from_bytes(original.expose())?)?;
    assert!(matches!(
        repository.import_recovery_key(&secrets, PayloadKey::generate()),
        Err(RepositoryError::RecoveryKeyMismatch)
    ));
    let after = repository
        .export_recovery_key(&secrets)?
        .ok_or("recovery key should still exist")?;
    assert_eq!(after.expose(), original.expose());
    Ok(())
}

#[test]
fn encrypted_capture_populates_the_recovery_wrapped_key_field() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let secrets = MemorySecrets::default();
    repository.configure_recovery_key(&secrets)?;
    let staging = repository.begin_staging(RunId::parse("run-recovery-wrap")?)?;
    let path = PayloadPath::parse("payload/filesystem.tar.zst.enc")?;
    staging.write_payload("filesystem", path.clone(), "application/zstd", b"plaintext")?;
    let payload = staging.encrypt_and_register_payload_file(
        "filesystem",
        path,
        "application/zstd",
        &BackupId::parse("backup-recovery-wrap")?,
        &secrets,
    )?;
    let encryption = payload.encryption.ok_or("payload should be encrypted")?;
    assert!(encryption.recovery_wrapped_key()?.is_some());
    Ok(())
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
