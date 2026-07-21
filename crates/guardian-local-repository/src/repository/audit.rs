use super::LocalRepository;
use crate::RepositoryError;
use crate::filesystem::{ensure_directory, sync_parent, write_new};
use guardian_core::{BackupId, ProfileId, RunId};
use serde::Serialize;

impl LocalRepository {
    pub fn write_capture_audit(
        &self,
        run_id: &RunId,
        state: &'static str,
        backup_id: Option<&BackupId>,
    ) -> Result<(), RepositoryError> {
        ensure_directory(&self.audit_root())?;
        let record = CaptureAuditRecord {
            state,
            run_id,
            backup_id,
        };
        let path = self
            .audit_root()
            .join(format!("capture-{run_id}-{state}.json"));
        let bytes = serde_json::to_vec(&record).map_err(|_| RepositoryError::Serialization)?;
        write_new(&path, &bytes)?;
        sync_parent(&path)
    }

    pub fn write_deploy_audit(
        &self,
        run_id: &RunId,
        state: &'static str,
        backup_id: &BackupId,
        target_profile_id: &ProfileId,
    ) -> Result<(), RepositoryError> {
        ensure_directory(&self.audit_root())?;
        let record = DeployAuditRecord {
            state,
            run_id,
            backup_id,
            target_profile_id,
        };
        let path = self
            .audit_root()
            .join(format!("deploy-{run_id}-{state}.json"));
        let bytes = serde_json::to_vec(&record).map_err(|_| RepositoryError::Serialization)?;
        write_new(&path, &bytes)?;
        sync_parent(&path)
    }

    pub fn write_replacement_audit(
        &self,
        run_id: &RunId,
        state: &'static str,
        backup_id: &BackupId,
        safety_backup_id: &BackupId,
        target_profile_id: &ProfileId,
    ) -> Result<(), RepositoryError> {
        ensure_directory(&self.audit_root())?;
        let record = ReplacementAuditRecord {
            state,
            run_id,
            backup_id,
            safety_backup_id,
            target_profile_id,
        };
        let path = self
            .audit_root()
            .join(format!("replacement-{run_id}-{state}.json"));
        let bytes = serde_json::to_vec(&record).map_err(|_| RepositoryError::Serialization)?;
        write_new(&path, &bytes)?;
        sync_parent(&path)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureAuditRecord<'a> {
    state: &'static str,
    run_id: &'a RunId,
    backup_id: Option<&'a BackupId>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeployAuditRecord<'a> {
    state: &'static str,
    run_id: &'a RunId,
    backup_id: &'a BackupId,
    target_profile_id: &'a ProfileId,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReplacementAuditRecord<'a> {
    state: &'static str,
    run_id: &'a RunId,
    backup_id: &'a BackupId,
    safety_backup_id: &'a BackupId,
    target_profile_id: &'a ProfileId,
}
