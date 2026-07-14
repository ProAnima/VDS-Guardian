use crate::filesystem::{ensure_directory, sync_parent, write_new};
use crate::inventory::{TrustedBackup, trusted_inventory};
use crate::retention_journal::RetentionTransaction;
use crate::{LocalRepository, RepositoryError};
use guardian_core::{
    BackupId, ManifestVerifier, PlanId, RepositoryId, RetentionOutcome, RetentionPlan,
    RetentionPolicy, RetentionSnapshotEntry, build_retention_plan,
};
use serde::Serialize;
use std::fs;
use std::path::PathBuf;

impl LocalRepository {
    pub fn plan_retention(
        &self,
        policy: RetentionPolicy,
        verifier: &dyn ManifestVerifier,
    ) -> Result<RetentionPlan, RepositoryError> {
        let _lock = self.acquire_lock()?;
        let inventory = trusted_inventory(&self.backups_root(), verifier)?;
        build_plan(self.id().clone(), policy, &inventory)
    }

    pub fn execute_retention(
        &self,
        plan: &RetentionPlan,
        confirmation: &str,
        verifier: &dyn ManifestVerifier,
    ) -> Result<RetentionOutcome, RepositoryError> {
        let _lock = self.acquire_lock()?;
        if plan.repository_id() != self.id() {
            return Err(RepositoryError::RepositoryMismatch);
        }
        let inventory = trusted_inventory(&self.backups_root(), verifier)?;
        let current = build_plan(self.id().clone(), plan.policy(), &inventory)?;
        if current != *plan {
            return Err(RepositoryError::SnapshotChanged);
        }
        if plan.delete_backup_ids().is_empty() {
            return Ok(RetentionOutcome {
                deleted_backups: 0,
                retained_backups: inventory.len(),
            });
        }
        if confirmation != plan.confirmation_phrase() {
            return Err(RepositoryError::ConfirmationMismatch);
        }
        self.execute_moves(plan)?;
        Ok(RetentionOutcome {
            deleted_backups: plan.delete_backup_ids().len(),
            retained_backups: inventory.len() - plan.delete_backup_ids().len(),
        })
    }

    fn execute_moves(&self, plan: &RetentionPlan) -> Result<(), RepositoryError> {
        write_audit(self.audit_root(), plan, "approved")?;
        let transaction = RetentionTransaction::begin(self, plan)?;
        let mut moved = Vec::new();
        for backup_id in plan.delete_backup_ids() {
            let source = self.backups_root().join(backup_id.as_str());
            let destination = transaction.trash().join(backup_id.as_str());
            if let Err(source_error) = fs::rename(&source, &destination) {
                rollback_moves(&moved)?;
                transaction.abort()?;
                return Err(RepositoryError::io(
                    "move backup to retention quarantine",
                    source_error,
                ));
            }
            moved.push((source, destination));
            if let Err(sync_error) = sync_move(&moved) {
                rollback_moves(&moved)?;
                transaction.abort()?;
                return Err(sync_error);
            }
        }
        transaction.mark_cleanup_ready()?;
        transaction.cleanup(self)?;
        write_audit(self.audit_root(), plan, "completed")?;
        transaction.finish()
    }
}

fn build_plan(
    repository_id: RepositoryId,
    policy: RetentionPolicy,
    inventory: &[TrustedBackup],
) -> Result<RetentionPlan, RepositoryError> {
    let snapshot = inventory
        .iter()
        .map(|backup| RetentionSnapshotEntry {
            backup_id: backup.backup_id.clone(),
            sealed_at: backup.sealed_at.clone(),
        })
        .collect();
    build_retention_plan(repository_id, policy, snapshot)
        .map_err(|_| RepositoryError::IntegrityFailure)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuditRecord<'a> {
    state: &'a str,
    plan_id: &'a PlanId,
    repository_id: &'a RepositoryId,
    delete_backup_ids: &'a [BackupId],
}

fn write_audit(root: PathBuf, plan: &RetentionPlan, state: &str) -> Result<(), RepositoryError> {
    write_retention_audit(
        root,
        plan.plan_id(),
        plan.repository_id(),
        plan.delete_backup_ids(),
        state,
    )
}

pub(crate) fn write_retention_audit(
    root: PathBuf,
    plan_id: &PlanId,
    repository_id: &RepositoryId,
    delete_backup_ids: &[BackupId],
    state: &str,
) -> Result<(), RepositoryError> {
    ensure_directory(&root)?;
    let path = root.join(format!("retention-{plan_id}-{state}.json"));
    let record = AuditRecord {
        state,
        plan_id,
        repository_id,
        delete_backup_ids,
    };
    let bytes = serde_json::to_vec(&record).map_err(|_| RepositoryError::Serialization)?;
    match write_new(&path, &bytes) {
        Err(RepositoryError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::AlreadyExists =>
        {
            Err(RepositoryError::AuditConflict)
        }
        result => {
            result?;
            sync_parent(&path)
        }
    }
}

fn rollback_moves(moved: &[(PathBuf, PathBuf)]) -> Result<(), RepositoryError> {
    for (source, destination) in moved.iter().rev() {
        if fs::rename(destination, source).is_err() {
            return Err(RepositoryError::RecoveryRequired);
        }
        sync_parent(source)?;
        sync_parent(destination)?;
    }
    Ok(())
}

fn sync_move(moved: &[(PathBuf, PathBuf)]) -> Result<(), RepositoryError> {
    let (source, destination) = moved.last().ok_or(RepositoryError::RecoveryRequired)?;
    sync_parent(source)?;
    sync_parent(destination)
}
