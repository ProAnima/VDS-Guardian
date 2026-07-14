use crate::{BackupId, PlanId, RepositoryId, Timestamp};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionPolicy {
    max_backups: usize,
    minimum_backups: usize,
}

impl RetentionPolicy {
    pub fn new(max_backups: usize, minimum_backups: usize) -> Result<Self, RetentionPolicyError> {
        if max_backups == 0 {
            return Err(RetentionPolicyError::EmptyRepositoryAllowed);
        }
        if minimum_backups > max_backups {
            return Err(RetentionPolicyError::MinimumExceedsMaximum);
        }
        Ok(Self {
            max_backups,
            minimum_backups,
        })
    }

    #[must_use]
    pub fn max_backups(self) -> usize {
        self.max_backups
    }

    #[must_use]
    pub fn minimum_backups(self) -> usize {
        self.minimum_backups
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum RetentionPolicyError {
    #[error("retention must preserve at least one backup")]
    EmptyRepositoryAllowed,
    #[error("retention minimum cannot exceed its maximum")]
    MinimumExceedsMaximum,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionPlan {
    plan_id: PlanId,
    repository_id: RepositoryId,
    policy: RetentionPolicy,
    snapshot: Vec<RetentionSnapshotEntry>,
    delete_backup_ids: Vec<BackupId>,
}

impl RetentionPlan {
    #[must_use]
    pub fn plan_id(&self) -> &PlanId {
        &self.plan_id
    }
    #[must_use]
    pub fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }
    #[must_use]
    pub fn policy(&self) -> RetentionPolicy {
        self.policy
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionSnapshotEntry {
    pub backup_id: BackupId,
    pub sealed_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionOutcome {
    pub deleted_backups: usize,
    pub retained_backups: usize,
}

pub fn build_retention_plan(
    repository_id: RepositoryId,
    policy: RetentionPolicy,
    snapshot: Vec<RetentionSnapshotEntry>,
) -> Result<RetentionPlan, RetentionPlanError> {
    let delete_count = snapshot.len().saturating_sub(policy.max_backups());
    if delete_count > 0 && snapshot.len() - delete_count < policy.minimum_backups() {
        return Err(RetentionPlanError::Integrity);
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
    let bytes = serde_json::to_vec(&digest).map_err(|_| RetentionPlanError::Serialization)?;
    let plan_id = PlanId::parse(hex(&Sha256::digest(bytes)))
        .map_err(|_| RetentionPlanError::Serialization)?;
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
struct PlanDigest<'a> {
    repository_id: &'a RepositoryId,
    policy: RetentionPolicy,
    snapshot: &'a [RetentionSnapshotEntry],
    delete_backup_ids: &'a [BackupId],
}

fn hex(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(ALPHABET[usize::from(byte >> 4)]));
        output.push(char::from(ALPHABET[usize::from(byte & 0x0f)]));
    }
    output
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum RetentionPlanError {
    #[error("retention policy would violate its preserved-backup invariant")]
    Integrity,
    #[error("retention plan could not be serialized")]
    Serialization,
}

#[cfg(test)]
mod tests {
    use super::{
        RetentionPolicy, RetentionPolicyError, RetentionSnapshotEntry, build_retention_plan,
    };
    use crate::{BackupId, RepositoryId, Timestamp};

    #[test]
    fn policy_rejects_destructive_bounds() {
        assert_eq!(
            RetentionPolicy::new(0, 0),
            Err(RetentionPolicyError::EmptyRepositoryAllowed)
        );
        assert_eq!(
            RetentionPolicy::new(2, 3),
            Err(RetentionPolicyError::MinimumExceedsMaximum)
        );
        assert!(RetentionPolicy::new(3, 2).is_ok());
    }

    #[test]
    fn retention_use_case_selects_only_the_oldest_backup_outside_the_limit()
    -> Result<(), Box<dyn std::error::Error>> {
        let snapshot = ["backup-001", "backup-002", "backup-003"]
            .into_iter()
            .map(|backup_id| {
                Ok(RetentionSnapshotEntry {
                    backup_id: BackupId::parse(backup_id)?,
                    sealed_at: Timestamp::parse("2026-07-14T12:00:00Z")?,
                })
            })
            .collect::<Result<Vec<_>, crate::IdentifierError>>()?;
        let plan = build_retention_plan(
            RepositoryId::parse("repository-001")?,
            RetentionPolicy::new(2, 1)?,
            snapshot,
        )?;
        assert_eq!(plan.delete_backup_ids()[0].as_str(), "backup-001");
        assert_eq!(plan.retained_backup_ids().len(), 2);
        Ok(())
    }
}
