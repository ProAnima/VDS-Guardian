use guardian_archive::{ArchiveLimits, inspect_tar_zstd};
use guardian_core::{
    BackupId, DockerInventoryPort, ManifestVerifier, ProfileId, RunId, SecretStore,
    SourceReplacementPlan, SourceReplacementPlanError, VdsProfile,
};
use guardian_local_repository::LocalRepository;
use guardian_ssh::SshError;
use guardian_ssh::{PinnedHost, ReplacementTarget, SshIdentity, SshUser, SystemOpenSsh};
use std::io::{Seek, SeekFrom};
use thiserror::Error;

pub struct ReplacementComposition<'a> {
    pub repository: &'a LocalRepository,
    pub ssh: &'a SystemOpenSsh,
    pub target_profile: &'a VdsProfile,
    pub credentials: &'a dyn SecretStore,
    pub verifier: &'a dyn ManifestVerifier,
    pub docker_inventory: &'a dyn DockerInventoryPort,
}

impl ReplacementComposition<'_> {
    pub fn plan(&self, backup_id: &BackupId) -> Result<SourceReplacementPlan, ReplacementError> {
        let manifest = self
            .repository
            .load_verified_manifest(backup_id, self.verifier)
            .map_err(|_| ReplacementError::Storage)?;
        let plan = SourceReplacementPlan::build(&manifest, self.target_profile)?;
        let inventory = if manifest
            .source_layout
            .as_ref()
            .is_some_and(|layout| !layout.docker_workloads.is_empty())
        {
            Some(
                self.docker_inventory
                    .inspect(self.target_profile)
                    .map_err(|_| ReplacementError::LivePreflight)?,
            )
        } else {
            None
        };
        let plan = plan.reconcile_current(inventory.as_ref());
        let session = self.resolve_session()?;
        let ready = self
            .ssh
            .probe_replacement_ready(
                &session.0,
                &session.1,
                session.2.path(),
                plan.impact.root.as_str(),
            )
            .map_err(|_| ReplacementError::LivePreflight)?;
        Ok(plan.reconcile_source_ready(ready))
    }

    pub fn execute(
        &self,
        run_id: &RunId,
        expected_profile_id: &ProfileId,
        backup_id: &BackupId,
        safety_backup_id: &BackupId,
        confirmation: &str,
    ) -> Result<SourceReplacementPlan, ReplacementError> {
        self.write_audit(run_id, "attempted", backup_id, safety_backup_id)?;
        let result = self.execute_inner(run_id, expected_profile_id, backup_id, confirmation);
        match &result {
            Ok(_) => self.write_audit(run_id, "completed", backup_id, safety_backup_id)?,
            Err(ReplacementError::RolledBack) => {
                let _ = self.write_audit(run_id, "rolled_back", backup_id, safety_backup_id);
            }
            Err(_) if self.ssh.is_cancelled() => {
                let _ = self.write_audit(run_id, "cancelled", backup_id, safety_backup_id);
            }
            Err(_) => {
                let _ = self.write_audit(run_id, "failed", backup_id, safety_backup_id);
            }
        }
        result
    }

    fn execute_inner(
        &self,
        run_id: &RunId,
        expected_profile_id: &ProfileId,
        backup_id: &BackupId,
        confirmation: &str,
    ) -> Result<SourceReplacementPlan, ReplacementError> {
        if self.target_profile.profile_id != *expected_profile_id {
            return Err(ReplacementError::TargetMismatch);
        }
        let plan = self.plan(backup_id)?;
        plan.approve(confirmation)?;
        let (mut reader, expected_bytes) = self
            .repository
            .open_deploy_payload_reader(
                backup_id,
                &plan.filesystem_payload,
                self.verifier,
                self.credentials,
            )
            .map_err(|_| ReplacementError::Storage)?;
        inspect_tar_zstd(&mut reader, ArchiveLimits::conservative())
            .map_err(|_| ReplacementError::ArchivePolicy)?;
        reader
            .seek(SeekFrom::Start(0))
            .map_err(|_| ReplacementError::Storage)?;
        let session = self.resolve_session()?;
        let containers = plan.impact.containers.clone();
        let target = ReplacementTarget {
            source_root: plan.impact.root.as_str(),
            run_id,
            containers: &containers,
        };
        self.ssh
            .push_replacement_staging_to(
                &session.0,
                &session.1,
                session.2.path(),
                target,
                reader,
                expected_bytes,
            )
            .map_err(|_| ReplacementError::PushFailed)?;
        self.ssh
            .commit_replacement_to(&session.0, &session.1, session.2.path(), target)
            .map_err(|error| match error {
                SshError::ReplacementRolledBack => ReplacementError::RolledBack,
                SshError::ReplacementRollbackFailed => ReplacementError::RollbackFailed,
                _ => ReplacementError::CutoverFailed,
            })?;
        Ok(plan)
    }

    fn resolve_session(&self) -> Result<(PinnedHost, SshUser, SshIdentity), ReplacementError> {
        let host = PinnedHost::parse(
            &self.target_profile.endpoint.host,
            self.target_profile.endpoint.port,
            &self.target_profile.endpoint.host_pin.algorithm,
            &self.target_profile.endpoint.host_pin.public_key_base64,
        )
        .map_err(|_| ReplacementError::Credentials)?;
        let user = SshUser::parse(&self.target_profile.endpoint.user)
            .map_err(|_| ReplacementError::Credentials)?;
        let identity =
            SshIdentity::from_store(self.credentials, &self.target_profile.credential_id)
                .map_err(|_| ReplacementError::Credentials)?;
        Ok((host, user, identity))
    }

    fn write_audit(
        &self,
        run_id: &RunId,
        state: &'static str,
        backup_id: &BackupId,
        safety_backup_id: &BackupId,
    ) -> Result<(), ReplacementError> {
        self.repository
            .write_replacement_audit(
                run_id,
                state,
                backup_id,
                safety_backup_id,
                &self.target_profile.profile_id,
            )
            .map_err(|_| ReplacementError::Storage)
    }
}

#[derive(Debug, Error)]
pub enum ReplacementError {
    #[error("replacement plan was rejected")]
    Plan(#[from] SourceReplacementPlanError),
    #[error("repository or payload is unavailable")]
    Storage,
    #[error("target profile changed")]
    TargetMismatch,
    #[error("SSH credentials are unavailable")]
    Credentials,
    #[error("current Docker state could not be verified")]
    LivePreflight,
    #[error("archive policy rejected the payload")]
    ArchivePolicy,
    #[error("replacement staging failed")]
    PushFailed,
    #[error("managed cutover failed and rollback was attempted")]
    CutoverFailed,
    #[error("managed cutover failed and the original data was restored")]
    RolledBack,
    #[error("managed cutover failed and automatic rollback was incomplete")]
    RollbackFailed,
}
