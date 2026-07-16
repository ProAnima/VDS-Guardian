mod support;

use guardian_archive::TarZstdWriter;
use guardian_core::{ArchivePath, BackupId, CredentialId, PayloadPath, ProfileId, RunId};
use guardian_core::{SecretStore, SecretStoreError, SecretValue};
use std::io::{Cursor, Read};
use std::{collections::HashMap, sync::Mutex};
use support::{TestResult, TestRoot, TestSigner, manifest, repository, timestamp};

#[test]
fn open_deploy_payload_reader_re_verifies_the_manifest_fresh_on_every_call() -> TestResult {
    let root = TestRoot::new()?;
    let local_repository = repository(&root)?;
    let signer = TestSigner::new();
    let secrets = MemorySecrets::default();
    let backup_id = BackupId::parse("backup-deploy-reverify")?;
    let run = RunId::parse("run-deploy-reverify")?;
    let staging = local_repository.begin_staging(run.clone())?;
    let path = PayloadPath::parse("payload/filesystem.tar.zst.enc")?;
    staging.write_payload("filesystem", path.clone(), "application/zstd", &archive()?)?;
    let payload = staging.encrypt_and_register_payload_file(
        "filesystem",
        path.clone(),
        "application/zstd",
        &backup_id,
        &secrets,
    )?;
    let mut manifest = manifest("backup-deploy-reverify", run)?;
    manifest.add_payload(payload)?;
    staging.seal(manifest, timestamp("2026-07-15T09:00:00Z")?, &signer)?;

    // First call succeeds against the still-valid sealed backup.
    let (mut first, _) =
        local_repository.open_deploy_payload_reader(&backup_id, &path, &signer, &secrets)?;
    let mut plaintext = Vec::new();
    first.read_to_end(&mut plaintext)?;
    assert_eq!(plaintext, archive()?);
    drop(first);

    // Tamper with the on-disk signature after the first (successful) read.
    let signature_path = root
        .path()
        .join("backups")
        .join(backup_id.as_str())
        .join("manifest.sig");
    let mut bytes = std::fs::read(&signature_path)?;
    if let Some(last) = bytes.last_mut() {
        *last ^= 1;
    }
    std::fs::write(&signature_path, bytes)?;

    // The second call must re-verify from scratch and fail closed, even
    // though the first call against the same repository already succeeded.
    assert!(
        local_repository
            .open_deploy_payload_reader(&backup_id, &path, &signer, &secrets)
            .is_err()
    );
    Ok(())
}

#[test]
fn open_deploy_payload_reader_fails_closed_when_the_key_is_missing() -> TestResult {
    let root = TestRoot::new()?;
    let local_repository = repository(&root)?;
    let signer = TestSigner::new();
    let secrets = MemorySecrets::default();
    let backup_id = BackupId::parse("backup-deploy-missing-key")?;
    let run = RunId::parse("run-deploy-missing-key")?;
    let staging = local_repository.begin_staging(run.clone())?;
    let path = PayloadPath::parse("payload/filesystem.tar.zst.enc")?;
    staging.write_payload("filesystem", path.clone(), "application/zstd", &archive()?)?;
    let payload = staging.encrypt_and_register_payload_file(
        "filesystem",
        path.clone(),
        "application/zstd",
        &backup_id,
        &secrets,
    )?;
    let mut manifest = manifest("backup-deploy-missing-key", run)?;
    manifest.add_payload(payload)?;
    staging.seal(manifest, timestamp("2026-07-15T09:00:00Z")?, &signer)?;

    assert!(
        local_repository
            .open_deploy_payload_reader(&backup_id, &path, &signer, &NoopSecrets)
            .is_err()
    );
    Ok(())
}

#[test]
fn open_deploy_payload_reader_returns_the_exact_verified_bytes() -> TestResult {
    let root = TestRoot::new()?;
    let local_repository = repository(&root)?;
    let signer = TestSigner::new();
    let secrets = MemorySecrets::default();
    let backup_id = BackupId::parse("backup-deploy-length")?;
    let run = RunId::parse("run-deploy-length")?;
    let staging = local_repository.begin_staging(run.clone())?;
    let path = PayloadPath::parse("payload/filesystem.tar.zst.enc")?;
    let source_bytes = archive()?;
    staging.write_payload(
        "filesystem",
        path.clone(),
        "application/zstd",
        &source_bytes,
    )?;
    let payload = staging.encrypt_and_register_payload_file(
        "filesystem",
        path.clone(),
        "application/zstd",
        &backup_id,
        &secrets,
    )?;
    let mut manifest = manifest("backup-deploy-length", run)?;
    manifest.add_payload(payload)?;
    staging.seal(manifest, timestamp("2026-07-15T09:00:00Z")?, &signer)?;

    let (mut reader, byte_length) =
        local_repository.open_deploy_payload_reader(&backup_id, &path, &signer, &secrets)?;
    // Asserted on its own, before any read, so this cannot pass merely
    // because the reader happens to yield the right number of bytes.
    assert_eq!(byte_length, source_bytes.len() as u64);
    let mut plaintext = Vec::new();
    reader.read_to_end(&mut plaintext)?;
    assert_eq!(plaintext.len() as u64, byte_length);
    assert_eq!(plaintext, source_bytes);
    Ok(())
}

#[test]
fn write_deploy_audit_records_distinct_atomically_written_states() -> TestResult {
    let root = TestRoot::new()?;
    let local_repository = repository(&root)?;
    let run_id = RunId::parse("run-deploy-audit")?;
    let backup_id = BackupId::parse("backup-deploy-audit")?;
    let target_profile_id = ProfileId::parse("profile-deploy-target")?;
    for state in ["attempted", "completed", "failed"] {
        local_repository.write_deploy_audit(&run_id, state, &backup_id, &target_profile_id)?;
    }
    for state in ["attempted", "completed", "failed"] {
        assert!(
            root.path()
                .join("audit")
                .join(format!("deploy-{}-{state}.json", run_id.as_str()))
                .exists()
        );
    }
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
