mod support;

use guardian_core::{BackupId, PayloadPath, RunId};
use guardian_local_repository::RepositoryError;
use support::{TestResult, TestRoot, TestSigner, manifest, repository, timestamp};

#[test]
fn restore_plan_rechecks_the_sealed_backup_and_requires_a_new_target() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let signer = TestSigner::new();
    let run = RunId::parse("run-restore")?;
    let staging = repository.begin_staging(run.clone())?;
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
    let plan =
        repository.plan_restore(&BackupId::parse("backup-restore")?, &destination, &signer)?;
    assert_eq!(plan.backup_id.as_str(), "backup-restore");
    std::fs::create_dir(&destination)?;
    assert!(matches!(
        repository.plan_restore(&BackupId::parse("backup-restore")?, &destination, &signer),
        Err(RepositoryError::RestoreDestinationExists)
    ));
    Ok(())
}
