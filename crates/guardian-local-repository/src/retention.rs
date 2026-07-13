use crate::filesystem::{ensure_directory, sync_parent, write_new};
use crate::inventory::{TrustedBackup, trusted_inventory};
use crate::{LocalRepository, RepositoryError};
use guardian_core::{BackupId, ManifestVerifier, PlanId, RepositoryId, RetentionPolicy, Timestamp};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionPlan {
    plan_id: PlanId,
    repository_id: RepositoryId,
    policy: RetentionPolicy,
    snapshot: Vec<SnapshotEntry>,
    delete_backup_ids: Vec<BackupId>,
}

impl RetentionPlan {
    #[must_use]
    pub fn plan_id(&self) -> &PlanId {
        &self.plan_id
    }

    #[must_use]
    pub fn delete_backup_ids(&self) -> &[BackupId] {
        &self.delete_backup_ids
    }

    #[must_use]
    pub fn retained_backup_ids(&self) -> Vec<&BackupId> {
        self.snapshot
            .iter()
            .filter(|entry| !self.delete_backup_ids.contains(&entry.backup_id))
            .map(|entry| &entry.backup_id)
            .collect()
    }

    #[must_use]
    pub fn confirmation_phrase(&self) -> String {
        format!(
            "DELETE {} BACKUPS {}",
            self.delete_backup_ids.len(),
            self.plan_id
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionOutcome {
    pub deleted_backups: usize,
    pub retained_backups: usize,
}

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
        if plan.repository_id != *self.id() {
            return Err(RepositoryError::RepositoryMismatch);
        }
        let inventory = trusted_inventory(&self.backups_root(), verifier)?;
        let current = build_plan(self.id().clone(), plan.policy, &inventory)?;
        if current != *plan {
            return Err(RepositoryError::SnapshotChanged);
        }
        if plan.delete_backup_ids.is_empty() {
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
            deleted_backups: plan.delete_backup_ids.len(),
            retained_backups: inventory.len() - plan.delete_backup_ids.len(),
        })
    }

    fn execute_moves(&self, plan: &RetentionPlan) -> Result<(), RepositoryError> {
        write_audit(self.audit_root(), plan, "approved")?;
        let trash = self
            .quarantine_root()
            .join(format!("retention-{}", plan.plan_id));
        fs::create_dir(&trash).map_err(map_retention_directory_error)?;
        let mut moved = Vec::new();
        for backup_id in &plan.delete_backup_ids {
            let source = self.backups_root().join(backup_id.as_str());
            let destination = trash.join(backup_id.as_str());
            if let Err(source_error) = fs::rename(&source, &destination) {
                rollback_moves(&moved)?;
                return Err(RepositoryError::io(
                    "move backup to retention quarantine",
                    source_error,
                ));
            }
            moved.push((source, destination));
            if let Err(sync_error) = sync_move(&moved) {
                rollback_moves(&moved)?;
                return Err(sync_error);
            }
        }
        if fs::remove_dir_all(&trash).is_err() {
            return Err(RepositoryError::CleanupPending);
        }
        write_audit(self.audit_root(), plan, "completed")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotEntry {
    backup_id: BackupId,
    sealed_at: Timestamp,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlanDigest<'a> {
    repository_id: &'a RepositoryId,
    policy: RetentionPolicy,
    snapshot: &'a [SnapshotEntry],
    delete_backup_ids: &'a [BackupId],
}

fn build_plan(
    repository_id: RepositoryId,
    policy: RetentionPolicy,
    inventory: &[TrustedBackup],
) -> Result<RetentionPlan, RepositoryError> {
    let snapshot = inventory
        .iter()
        .map(|backup| SnapshotEntry {
            backup_id: backup.backup_id.clone(),
            sealed_at: backup.sealed_at.clone(),
        })
        .collect::<Vec<_>>();
    let delete_count = inventory.len().saturating_sub(policy.max_backups());
    if delete_count > 0 && inventory.len() - delete_count < policy.minimum_backups() {
        return Err(RepositoryError::IntegrityFailure);
    }
    let delete_backup_ids = snapshot
        .iter()
        .take(delete_count)
        .map(|entry| entry.backup_id.clone())
        .collect::<Vec<_>>();
    let digest = PlanDigest {
        repository_id: &repository_id,
        policy,
        snapshot: &snapshot,
        delete_backup_ids: &delete_backup_ids,
    };
    let bytes = serde_json::to_vec(&digest).map_err(|_| RepositoryError::Serialization)?;
    let plan_id = PlanId::parse(crate::verification::hex(&Sha256::digest(bytes)))
        .map_err(|_| RepositoryError::Serialization)?;
    Ok(RetentionPlan {
        plan_id,
        repository_id,
        policy,
        snapshot,
        delete_backup_ids,
    })
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
    ensure_directory(&root)?;
    let path = root.join(format!("retention-{}-{state}.json", plan.plan_id));
    let record = AuditRecord {
        state,
        plan_id: &plan.plan_id,
        repository_id: &plan.repository_id,
        delete_backup_ids: &plan.delete_backup_ids,
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

fn map_retention_directory_error(source: std::io::Error) -> RepositoryError {
    if source.kind() == std::io::ErrorKind::AlreadyExists {
        RepositoryError::AuditConflict
    } else {
        RepositoryError::io("create retention quarantine", source)
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
