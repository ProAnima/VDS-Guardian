mod support;

use guardian_core::{PayloadPath, RetentionPolicy, RunId};
use guardian_local_repository::{LocalRepository, RepositoryError};
use serde_json::json;
use std::fs;
use support::{TestResult, TestRoot, TestSigner, manifest, repository, timestamp};

#[test]
fn retention_deletes_only_the_oldest_whole_backup() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let signer = TestSigner::new();
    seal(&repository, &signer, 1, "2026-07-13T12:01:00Z")?;
    seal(&repository, &signer, 2, "2026-07-13T12:02:00Z")?;
    seal(&repository, &signer, 3, "2026-07-13T12:03:00Z")?;
    let survivor_before = fs::read(root.path().join("backups/backup-002/manifest.json"))?;

    let plan = repository.plan_retention(RetentionPolicy::new(2, 2)?, &signer)?;
    assert_eq!(ids(plan.delete_backup_ids()), ["backup-001"]);
    assert_eq!(
        plan.retained_backup_ids()
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>(),
        ["backup-002", "backup-003"]
    );
    let outcome = repository.execute_retention(&plan, &plan.confirmation_phrase(), &signer)?;

    assert_eq!(outcome.deleted_backups, 1);
    assert_eq!(outcome.retained_backups, 2);
    assert!(!root.path().join("backups/backup-001").exists());
    assert!(root.path().join("backups/backup-002").is_dir());
    assert_eq!(
        fs::read(root.path().join("backups/backup-002/manifest.json"))?,
        survivor_before
    );
    assert!(audit_path(&root, &plan, "approved").is_file());
    assert!(audit_path(&root, &plan, "completed").is_file());
    Ok(())
}

#[test]
fn exact_confirmation_is_required_before_any_move() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let signer = TestSigner::new();
    seal(&repository, &signer, 1, "2026-07-13T12:01:00Z")?;
    seal(&repository, &signer, 2, "2026-07-13T12:02:00Z")?;
    let plan = repository.plan_retention(RetentionPolicy::new(1, 1)?, &signer)?;

    let result = repository.execute_retention(&plan, "DELETE", &signer);
    assert!(matches!(result, Err(RepositoryError::ConfirmationMismatch)));
    assert!(root.path().join("backups/backup-001").is_dir());
    assert!(!audit_path(&root, &plan, "approved").exists());
    Ok(())
}

#[test]
fn changed_snapshot_invalidates_an_approved_plan() -> TestResult {
    let root = TestRoot::new()?;
    let local_repository = repository(&root)?;
    let signer = TestSigner::new();
    seal(&local_repository, &signer, 1, "2026-07-13T12:01:00Z")?;
    seal(&local_repository, &signer, 2, "2026-07-13T12:02:00Z")?;
    let plan = local_repository.plan_retention(RetentionPolicy::new(1, 1)?, &signer)?;
    seal(&local_repository, &signer, 3, "2026-07-13T12:03:00Z")?;

    let result = local_repository.execute_retention(&plan, &plan.confirmation_phrase(), &signer);
    assert!(matches!(result, Err(RepositoryError::SnapshotChanged)));
    for index in 1..=3 {
        assert!(
            root.path()
                .join(format!("backups/backup-{index:03}"))
                .is_dir()
        );
    }
    Ok(())
}

#[test]
fn tampering_blocks_retention_planning() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let signer = TestSigner::new();
    seal(&repository, &signer, 1, "2026-07-13T12:01:00Z")?;
    fs::write(
        root.path()
            .join("backups/backup-001/payload/filesystem.tar.zst"),
        b"tampered",
    )?;

    let result = repository.plan_retention(RetentionPolicy::new(1, 1)?, &signer);
    assert!(matches!(result, Err(RepositoryError::IntegrityFailure)));
    assert!(root.path().join("backups/backup-001").is_dir());
    Ok(())
}

#[test]
fn forged_signature_blocks_retention_planning() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let signer = TestSigner::new();
    seal(&repository, &signer, 1, "2026-07-13T12:01:00Z")?;
    let signature = root.path().join("backups/backup-001/manifest.sig");
    let mut forged: serde_json::Value = serde_json::from_slice(&fs::read(&signature)?)?;
    forged["signature"] = serde_json::Value::String("00".repeat(64));
    fs::write(signature, serde_json::to_vec(&forged)?)?;

    let result = repository.plan_retention(RetentionPolicy::new(1, 1)?, &signer);
    assert!(matches!(result, Err(RepositoryError::Signing(_))));
    assert!(root.path().join("backups/backup-001").is_dir());
    Ok(())
}

#[test]
fn no_op_plan_needs_no_destructive_confirmation() -> TestResult {
    let root = TestRoot::new()?;
    let repository = repository(&root)?;
    let signer = TestSigner::new();
    seal(&repository, &signer, 1, "2026-07-13T12:01:00Z")?;
    let plan = repository.plan_retention(RetentionPolicy::new(2, 2)?, &signer)?;

    assert!(plan.delete_backup_ids().is_empty());
    let outcome = repository.execute_retention(&plan, "", &signer)?;
    assert_eq!(outcome.deleted_backups, 0);
    assert_eq!(outcome.retained_backups, 1);
    Ok(())
}

#[test]
fn reopening_rolls_back_a_retention_move_interrupted_before_cleanup() -> TestResult {
    let root = TestRoot::new()?;
    let local_repository = repository(&root)?;
    let signer = TestSigner::new();
    seal(&local_repository, &signer, 1, "2026-07-13T12:01:00Z")?;
    seal(&local_repository, &signer, 2, "2026-07-13T12:02:00Z")?;
    let plan = local_repository.plan_retention(RetentionPolicy::new(1, 1)?, &signer)?;
    write_retention_intent(&root, &plan, false)?;
    move_selected_to_trash(&root, &plan)?;

    drop(local_repository);
    let reopened = repository(&root)?;

    assert!(root.path().join("backups/backup-001").is_dir());
    assert!(!retention_trash(&root, &plan).exists());
    assert!(!retention_intent(&root, &plan).exists());
    let rebuilt = reopened.plan_retention(RetentionPolicy::new(1, 1)?, &signer)?;
    assert_eq!(ids(rebuilt.delete_backup_ids()), ["backup-001"]);
    Ok(())
}

#[test]
fn reopening_finishes_a_retention_cleanup_interrupted_after_approval() -> TestResult {
    let root = TestRoot::new()?;
    let local_repository = repository(&root)?;
    let signer = TestSigner::new();
    seal(&local_repository, &signer, 1, "2026-07-13T12:01:00Z")?;
    seal(&local_repository, &signer, 2, "2026-07-13T12:02:00Z")?;
    let plan = local_repository.plan_retention(RetentionPolicy::new(1, 1)?, &signer)?;
    write_retention_intent(&root, &plan, true)?;
    move_selected_to_trash(&root, &plan)?;

    drop(local_repository);
    let _reopened = repository(&root)?;

    assert!(!root.path().join("backups/backup-001").exists());
    assert!(!retention_trash(&root, &plan).exists());
    assert!(!retention_intent(&root, &plan).exists());
    assert!(audit_path(&root, &plan, "completed").is_file());
    Ok(())
}

#[test]
fn reopening_resumes_a_partially_completed_retention_cleanup() -> TestResult {
    let root = TestRoot::new()?;
    let local_repository = repository(&root)?;
    let signer = TestSigner::new();
    for index in 1..=3 {
        seal(
            &local_repository,
            &signer,
            index,
            &format!("2026-07-13T12:0{index}:00Z"),
        )?;
    }
    let plan = local_repository.plan_retention(RetentionPolicy::new(1, 1)?, &signer)?;
    write_retention_intent(&root, &plan, true)?;
    move_selected_to_trash(&root, &plan)?;
    fs::remove_dir_all(retention_trash(&root, &plan).join("backup-001"))?;

    drop(local_repository);
    let _reopened = repository(&root)?;

    assert!(!root.path().join("backups/backup-001").exists());
    assert!(!root.path().join("backups/backup-002").exists());
    assert!(root.path().join("backups/backup-003").is_dir());
    assert!(!retention_intent(&root, &plan).exists());
    Ok(())
}

#[test]
fn reopening_completes_journal_cleanup_after_the_completed_audit() -> TestResult {
    let root = TestRoot::new()?;
    let local_repository = repository(&root)?;
    let signer = TestSigner::new();
    seal(&local_repository, &signer, 1, "2026-07-13T12:01:00Z")?;
    seal(&local_repository, &signer, 2, "2026-07-13T12:02:00Z")?;
    let plan = local_repository.plan_retention(RetentionPolicy::new(1, 1)?, &signer)?;
    write_retention_intent(&root, &plan, false)?;
    move_selected_to_trash(&root, &plan)?;
    fs::remove_dir_all(retention_trash(&root, &plan))?;
    write_completed_audit(&root, &plan)?;

    drop(local_repository);
    let _reopened = repository(&root)?;

    assert!(!root.path().join("backups/backup-001").exists());
    assert!(!retention_intent(&root, &plan).exists());
    Ok(())
}

#[test]
fn orphaned_retention_quarantine_fails_closed_on_open() -> TestResult {
    let root = TestRoot::new()?;
    let local_repository = repository(&root)?;
    let signer = TestSigner::new();
    seal(&local_repository, &signer, 1, "2026-07-13T12:01:00Z")?;
    seal(&local_repository, &signer, 2, "2026-07-13T12:02:00Z")?;
    let plan = local_repository.plan_retention(RetentionPolicy::new(1, 1)?, &signer)?;
    fs::create_dir(retention_trash(&root, &plan))?;

    drop(local_repository);
    let reopened = repository(&root);

    assert!(
        matches!(reopened, Err(error) if error.downcast_ref::<RepositoryError>().is_some_and(|value| matches!(value, RepositoryError::RecoveryRequired)))
    );
    Ok(())
}

fn seal(
    repository: &LocalRepository,
    signer: &TestSigner,
    index: usize,
    sealed_at: &str,
) -> TestResult {
    let run_id = RunId::parse(format!("run-{index:03}"))?;
    let staging = repository.begin_staging(run_id.clone())?;
    let payload = staging.write_payload(
        "filesystem",
        PayloadPath::parse("payload/filesystem.tar.zst")?,
        "application/zstd",
        format!("backup payload {index}").as_bytes(),
    )?;
    let mut manifest = manifest(&format!("backup-{index:03}"), run_id)?;
    manifest.add_payload(payload)?;
    staging.seal(manifest, timestamp(sealed_at)?, signer)?;
    Ok(())
}

fn ids(values: &[guardian_core::BackupId]) -> Vec<&str> {
    values.iter().map(guardian_core::BackupId::as_str).collect()
}

fn audit_path(
    root: &TestRoot,
    plan: &guardian_local_repository::RetentionPlan,
    state: &str,
) -> std::path::PathBuf {
    root.path()
        .join(format!("audit/retention-{}-{state}.json", plan.plan_id()))
}

fn write_retention_intent(
    root: &TestRoot,
    plan: &guardian_local_repository::RetentionPlan,
    cleanup_ready: bool,
) -> TestResult {
    let intent = json!({
        "formatVersion": 1,
        "planId": plan.plan_id().as_str(),
        "repositoryId": "repository-test",
        "deleteBackupIds": plan.delete_backup_ids().iter().map(|id| id.as_str()).collect::<Vec<_>>(),
    });
    fs::write(retention_intent(root, plan), serde_json::to_vec(&intent)?)?;
    if cleanup_ready {
        fs::write(retention_cleanup_marker(root, plan), b"")?;
    }
    Ok(())
}

fn write_completed_audit(
    root: &TestRoot,
    plan: &guardian_local_repository::RetentionPlan,
) -> TestResult {
    let audit = json!({
        "state": "completed",
        "planId": plan.plan_id().as_str(),
        "repositoryId": "repository-test",
        "deleteBackupIds": plan.delete_backup_ids().iter().map(|id| id.as_str()).collect::<Vec<_>>(),
    });
    fs::write(
        audit_path(root, plan, "completed"),
        serde_json::to_vec(&audit)?,
    )?;
    Ok(())
}

fn move_selected_to_trash(
    root: &TestRoot,
    plan: &guardian_local_repository::RetentionPlan,
) -> TestResult {
    let trash = retention_trash(root, plan);
    fs::create_dir(&trash)?;
    for backup_id in plan.delete_backup_ids() {
        fs::rename(
            root.path().join("backups").join(backup_id.as_str()),
            trash.join(backup_id.as_str()),
        )?;
    }
    Ok(())
}

fn retention_trash(
    root: &TestRoot,
    plan: &guardian_local_repository::RetentionPlan,
) -> std::path::PathBuf {
    root.path()
        .join(format!("quarantine/retention-{}", plan.plan_id()))
}

fn retention_intent(
    root: &TestRoot,
    plan: &guardian_local_repository::RetentionPlan,
) -> std::path::PathBuf {
    root.path().join(format!(
        "quarantine/retention-{}.intent.json",
        plan.plan_id()
    ))
}

fn retention_cleanup_marker(
    root: &TestRoot,
    plan: &guardian_local_repository::RetentionPlan,
) -> std::path::PathBuf {
    root.path().join(format!(
        "quarantine/retention-{}.cleanup-ready",
        plan.plan_id()
    ))
}
