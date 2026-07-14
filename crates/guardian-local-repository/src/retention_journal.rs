use crate::filesystem::{ensure_directory, sync_parent, write_new};
use crate::retention::write_retention_audit;
use crate::{LocalRepository, RepositoryError, RetentionPlan};
use guardian_core::{BackupId, PlanId, RepositoryId};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const INTENT_SUFFIX: &str = ".intent.json";
const CLEANUP_SUFFIX: &str = ".cleanup-ready";
const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RetentionIntent {
    format_version: u32,
    plan_id: PlanId,
    repository_id: RepositoryId,
    delete_backup_ids: Vec<BackupId>,
}

impl RetentionIntent {
    fn from_plan(plan: &RetentionPlan) -> Self {
        Self {
            format_version: FORMAT_VERSION,
            plan_id: plan.plan_id().clone(),
            repository_id: plan.repository_id().clone(),
            delete_backup_ids: plan.delete_backup_ids().to_vec(),
        }
    }

    fn validate(
        &self,
        expected_plan_id: &PlanId,
        repository_id: &RepositoryId,
    ) -> Result<(), RepositoryError> {
        if self.format_version != FORMAT_VERSION
            || self.plan_id != *expected_plan_id
            || self.repository_id != *repository_id
            || self.delete_backup_ids.is_empty()
            || has_duplicate_ids(&self.delete_backup_ids)
        {
            return Err(RepositoryError::RecoveryRequired);
        }
        Ok(())
    }
}

pub(crate) struct RetentionTransaction {
    intent: RetentionIntent,
    intent_path: PathBuf,
    cleanup_path: PathBuf,
    trash: PathBuf,
}

impl RetentionTransaction {
    pub(crate) fn begin(
        repository: &LocalRepository,
        plan: &RetentionPlan,
    ) -> Result<Self, RepositoryError> {
        let intent = RetentionIntent::from_plan(plan);
        let prefix = transaction_prefix(intent.plan_id.as_str());
        let root = repository.quarantine_root();
        let transaction = Self {
            intent,
            intent_path: root.join(format!("{prefix}{INTENT_SUFFIX}")),
            cleanup_path: root.join(format!("{prefix}{CLEANUP_SUFFIX}")),
            trash: root.join(prefix),
        };
        transaction.write_intent()?;
        fs::create_dir(&transaction.trash).map_err(map_create_error)?;
        sync_parent(&transaction.trash)?;
        Ok(transaction)
    }

    pub(crate) fn trash(&self) -> &Path {
        &self.trash
    }

    pub(crate) fn mark_cleanup_ready(&self) -> Result<(), RepositoryError> {
        write_new(&self.cleanup_path, b"")?;
        sync_parent(&self.cleanup_path)
    }

    pub(crate) fn cleanup(&self, repository: &LocalRepository) -> Result<(), RepositoryError> {
        cleanup_transaction(repository, &self.intent, &self.trash)
    }

    pub(crate) fn finish(&self) -> Result<(), RepositoryError> {
        remove_if_exists(&self.cleanup_path)?;
        remove_if_exists(&self.intent_path)
    }

    pub(crate) fn abort(&self) -> Result<(), RepositoryError> {
        remove_empty_if_exists(&self.trash)?;
        self.finish()
    }

    fn write_intent(&self) -> Result<(), RepositoryError> {
        let bytes = serde_json::to_vec(&self.intent).map_err(|_| RepositoryError::Serialization)?;
        write_new(&self.intent_path, &bytes)?;
        sync_parent(&self.intent_path)
    }
}

impl LocalRepository {
    pub(crate) fn reconcile_retention_locked(&self) -> Result<(), RepositoryError> {
        let intents = retention_intents(&self.quarantine_root())?;
        for intent_path in intents {
            reconcile_transaction(self, &intent_path)?;
        }
        reject_orphan_retention_entries(&self.quarantine_root())
    }
}

fn reconcile_transaction(
    repository: &LocalRepository,
    intent_path: &Path,
) -> Result<(), RepositoryError> {
    let plan_id = plan_id_from_intent_path(intent_path)?;
    let intent = read_intent(intent_path)?;
    intent.validate(&plan_id, repository.id())?;
    let root = repository.quarantine_root();
    let prefix = transaction_prefix(plan_id.as_str());
    let cleanup_path = root.join(format!("{prefix}{CLEANUP_SUFFIX}"));
    let trash = root.join(prefix);
    if cleanup_path.exists() {
        ensure_regular_file(&cleanup_path)?;
        cleanup_transaction(repository, &intent, &trash)?;
        complete_recovered_transaction(repository, &intent, &cleanup_path, intent_path)
    } else if completed_audit_exists(repository, &intent)? {
        cleanup_transaction(repository, &intent, &trash)?;
        remove_if_exists(intent_path)
    } else {
        rollback_transaction(repository, &intent, &trash)?;
        remove_if_exists(intent_path)
    }
}

fn cleanup_transaction(
    repository: &LocalRepository,
    intent: &RetentionIntent,
    trash: &Path,
) -> Result<(), RepositoryError> {
    validate_cleanup_state(repository, intent, trash)?;
    for backup_id in &intent.delete_backup_ids {
        let path = trash.join(backup_id.as_str());
        if path.exists() {
            fs::remove_dir_all(&path).map_err(|_| RepositoryError::CleanupPending)?;
            sync_parent(&path)?;
        }
    }
    remove_empty_if_exists(trash).map_err(|_| RepositoryError::CleanupPending)
}

fn rollback_transaction(
    repository: &LocalRepository,
    intent: &RetentionIntent,
    trash: &Path,
) -> Result<(), RepositoryError> {
    validate_rollback_state(repository, intent, trash)?;
    for backup_id in intent.delete_backup_ids.iter().rev() {
        let source = trash.join(backup_id.as_str());
        if source.exists() {
            let destination = repository.backups_root().join(backup_id.as_str());
            fs::rename(&source, &destination).map_err(|_| RepositoryError::RecoveryRequired)?;
            sync_parent(&source)?;
            sync_parent(&destination)?;
        }
    }
    remove_empty_if_exists(trash)
}

fn validate_cleanup_state(
    repository: &LocalRepository,
    intent: &RetentionIntent,
    trash: &Path,
) -> Result<(), RepositoryError> {
    validate_trash_entries(intent, trash)?;
    for backup_id in &intent.delete_backup_ids {
        if repository.backups_root().join(backup_id.as_str()).exists() {
            return Err(RepositoryError::RecoveryRequired);
        }
    }
    Ok(())
}

fn validate_rollback_state(
    repository: &LocalRepository,
    intent: &RetentionIntent,
    trash: &Path,
) -> Result<(), RepositoryError> {
    validate_trash_entries(intent, trash)?;
    for backup_id in &intent.delete_backup_ids {
        let active = repository.backups_root().join(backup_id.as_str());
        let quarantined = trash.join(backup_id.as_str());
        let active_exists = real_directory_or_absent(&active)?;
        let quarantined_exists = real_directory_or_absent(&quarantined)?;
        if active_exists == quarantined_exists {
            return Err(RepositoryError::RecoveryRequired);
        }
    }
    Ok(())
}

fn validate_trash_entries(intent: &RetentionIntent, trash: &Path) -> Result<(), RepositoryError> {
    if !trash.exists() {
        return Ok(());
    }
    ensure_directory(trash)?;
    for entry in fs::read_dir(trash)
        .map_err(|source| RepositoryError::io("list retention quarantine", source))?
    {
        let entry = entry
            .map_err(|source| RepositoryError::io("read retention quarantine entry", source))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| RepositoryError::UnsafeFilesystemEntry)?;
        let expected = intent
            .delete_backup_ids
            .iter()
            .any(|id| id.as_str() == name);
        if !expected || !real_directory_or_absent(&entry.path())? {
            return Err(RepositoryError::RecoveryRequired);
        }
    }
    Ok(())
}

fn complete_recovered_transaction(
    repository: &LocalRepository,
    intent: &RetentionIntent,
    cleanup_path: &Path,
    intent_path: &Path,
) -> Result<(), RepositoryError> {
    match write_retention_audit(
        repository.audit_root(),
        &intent.plan_id,
        &intent.repository_id,
        &intent.delete_backup_ids,
        "completed",
    ) {
        Ok(()) | Err(RepositoryError::AuditConflict) => {}
        Err(error) => return Err(error),
    }
    remove_if_exists(cleanup_path)?;
    remove_if_exists(intent_path)
}

fn completed_audit_exists(
    repository: &LocalRepository,
    intent: &RetentionIntent,
) -> Result<bool, RepositoryError> {
    let path = repository
        .audit_root()
        .join(format!("retention-{}-completed.json", intent.plan_id));
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(RepositoryError::RecoveryRequired),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(RepositoryError::io("inspect retention audit", source)),
    }
}

fn retention_intents(root: &Path) -> Result<Vec<PathBuf>, RepositoryError> {
    let mut intents = Vec::new();
    for entry in fs::read_dir(root)
        .map_err(|source| RepositoryError::io("list quarantine entries", source))?
    {
        let entry = entry.map_err(|source| RepositoryError::io("read quarantine entry", source))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| RepositoryError::UnsafeFilesystemEntry)?;
        if name.starts_with("retention-") && name.ends_with(INTENT_SUFFIX) {
            ensure_regular_file(&entry.path())?;
            intents.push(entry.path());
        }
    }
    Ok(intents)
}

fn reject_orphan_retention_entries(root: &Path) -> Result<(), RepositoryError> {
    for entry in fs::read_dir(root)
        .map_err(|source| RepositoryError::io("list quarantine entries", source))?
    {
        let entry = entry.map_err(|source| RepositoryError::io("read quarantine entry", source))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| RepositoryError::UnsafeFilesystemEntry)?;
        if name.starts_with("retention-") {
            return Err(RepositoryError::RecoveryRequired);
        }
    }
    Ok(())
}

fn read_intent(path: &Path) -> Result<RetentionIntent, RepositoryError> {
    ensure_regular_file(path)?;
    let bytes =
        fs::read(path).map_err(|source| RepositoryError::io("read retention intent", source))?;
    serde_json::from_slice(&bytes).map_err(|_| RepositoryError::RecoveryRequired)
}

fn plan_id_from_intent_path(path: &Path) -> Result<PlanId, RepositoryError> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(RepositoryError::UnsafeFilesystemEntry)?;
    let plan = name
        .strip_prefix("retention-")
        .and_then(|value| value.strip_suffix(INTENT_SUFFIX))
        .ok_or(RepositoryError::RecoveryRequired)?;
    PlanId::parse(plan).map_err(|_| RepositoryError::RecoveryRequired)
}

fn transaction_prefix(plan_id: &str) -> String {
    format!("retention-{plan_id}")
}

fn ensure_regular_file(path: &Path) -> Result<(), RepositoryError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| RepositoryError::io("inspect retention journal", source))?;
    if metadata.is_file() && !metadata.file_type().is_symlink() {
        Ok(())
    } else {
        Err(RepositoryError::UnsafeFilesystemEntry)
    }
}

fn real_directory_or_absent(path: &Path) -> Result<bool, RepositoryError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(RepositoryError::UnsafeFilesystemEntry),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(RepositoryError::io("inspect retention backup", source)),
    }
}

fn remove_if_exists(path: &Path) -> Result<(), RepositoryError> {
    match fs::remove_file(path) {
        Ok(()) => sync_parent(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(RepositoryError::io("remove retention journal", source)),
    }
}

fn remove_empty_if_exists(path: &Path) -> Result<(), RepositoryError> {
    match fs::remove_dir(path) {
        Ok(()) => sync_parent(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(RepositoryError::io("remove retention quarantine", source)),
    }
}

fn map_create_error(source: std::io::Error) -> RepositoryError {
    if source.kind() == std::io::ErrorKind::AlreadyExists {
        RepositoryError::AuditConflict
    } else {
        RepositoryError::io("create retention quarantine", source)
    }
}

fn has_duplicate_ids(ids: &[BackupId]) -> bool {
    ids.iter()
        .enumerate()
        .any(|(index, id)| ids[..index].contains(id))
}
