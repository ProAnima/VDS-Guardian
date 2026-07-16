//! Composition root for pushing a sealed backup onto a new/clean VDS over
//! SSH. Kept separate from `guardian-capture` (pull-only) so that every
//! crate capable of *mutating* a remote host is enumerable on its own — see
//! `docs/adr/0007-remote-deploy-to-a-new-vds.md`.

use guardian_core::{
    BackupId, DeploymentPlan, DeploymentPlanError, ManifestVerifier, PayloadPath, ProfileId,
    RemoteTargetPath, RunId, SecretStore, VdsProfile,
};
use guardian_local_repository::LocalRepository;
use guardian_ssh::{PinnedHost, SshIdentity, SshUser, StagingTarget, SystemOpenSsh};
use thiserror::Error;

pub struct DeploymentComposition<'a> {
    pub repository: &'a LocalRepository,
    pub ssh: &'a SystemOpenSsh,
    pub target_profile: &'a VdsProfile,
    pub credentials: &'a dyn SecretStore,
    pub verifier: &'a dyn ManifestVerifier,
}

impl DeploymentComposition<'_> {
    /// Builds a deployment plan and, unlike `RestorePlan::build`'s fully
    /// offline local restore counterpart, touches the network once to
    /// confirm the remote target is currently absent — early feedback
    /// before the operator ever types the confirmation phrase. The actual
    /// push commands re-check absence themselves regardless; this is a
    /// convenience, not the enforcement.
    pub fn plan(
        &self,
        backup_id: &BackupId,
        target_path: RemoteTargetPath,
    ) -> Result<DeploymentPlan, DeployError> {
        let manifest = self
            .repository
            .load_verified_manifest(backup_id, self.verifier)
            .map_err(|_| DeployError::Storage)?;
        let plan = DeploymentPlan::build(&manifest, self.target_profile, target_path)?;
        let session = self.resolve_ssh_session()?;
        let absent = self
            .ssh
            .probe_target_absent(
                &session.host,
                &session.user,
                session.identity.path(),
                plan.target_path.as_str(),
            )
            .map_err(|_| DeployError::PreflightFailed)?;
        if !absent {
            return Err(DeployError::TargetNotAbsent);
        }
        Ok(plan)
    }

    /// Writes the attempted/completed/cancelled/failed audit trail itself —
    /// mandatory, not a responsibility left to whichever caller happens to
    /// invoke this, per `CODEX.md`'s requirement that a destructive server
    /// mutation always carries an audit record. "Attempted" and "completed"
    /// are strict (a write failure here fails the call even though the push
    /// itself succeeded); "cancelled"/"failed" are best-effort, matching
    /// every caller's own prior behavior before this became the
    /// composition's job.
    pub fn execute(
        &self,
        run_id: &RunId,
        expected_target_profile_id: &ProfileId,
        backup_id: &BackupId,
        target_path: RemoteTargetPath,
        confirmation: &str,
    ) -> Result<DeploymentPlan, DeployError> {
        self.write_audit(run_id, "attempted", backup_id)?;
        let result = self.execute_pushes(
            run_id,
            expected_target_profile_id,
            backup_id,
            target_path,
            confirmation,
        );
        match &result {
            Ok(_) => self.write_audit(run_id, "completed", backup_id)?,
            Err(_) if self.ssh.is_cancelled() => {
                let _ = self.write_audit(run_id, "cancelled", backup_id);
            }
            Err(_) => {
                let _ = self.write_audit(run_id, "failed", backup_id);
            }
        }
        result
    }

    fn write_audit(
        &self,
        run_id: &RunId,
        state: &'static str,
        backup_id: &BackupId,
    ) -> Result<(), DeployError> {
        self.repository
            .write_deploy_audit(run_id, state, backup_id, &self.target_profile.profile_id)
            .map_err(|_| DeployError::Storage)
    }

    /// Re-derives the plan from scratch (never accepts one as trusted
    /// input), approves the confirmation, then pushes each payload with its
    /// own fresh manifest re-verification immediately beforehand — the
    /// filesystem and database pushes are each network-bound and can run
    /// for minutes, so the second push must not rely on a verification
    /// already minutes stale by the time it starts.
    ///
    /// A filesystem-only deploy is already fully atomic as a single push
    /// with an immediate remote rename — no second payload exists to race
    /// against, so it is unchanged. A combined deploy instead stages both
    /// payloads under one shared remote directory (neither push renames
    /// into place) and finalizes with one separate rename, so a failed
    /// second payload never leaves the first payload's content live at the
    /// target — see `docs/adr/0007-remote-deploy-to-a-new-vds.md`.
    fn execute_pushes(
        &self,
        run_id: &RunId,
        expected_target_profile_id: &ProfileId,
        backup_id: &BackupId,
        target_path: RemoteTargetPath,
        confirmation: &str,
    ) -> Result<DeploymentPlan, DeployError> {
        if self.target_profile.profile_id != *expected_target_profile_id {
            return Err(DeployError::TargetProfileMismatch);
        }
        let manifest = self
            .repository
            .load_verified_manifest(backup_id, self.verifier)
            .map_err(|_| DeployError::Storage)?;
        let plan = DeploymentPlan::build(&manifest, self.target_profile, target_path)?;
        plan.approve(confirmation)?;
        let session = self.resolve_ssh_session()?;

        match &plan.database_payload {
            None => {
                self.push_payload(
                    &session,
                    backup_id,
                    &plan.filesystem_payload,
                    plan.target_path.as_str(),
                    PushKind::FilesystemOnly,
                )?;
            }
            Some(database_payload) => {
                self.push_payload(
                    &session,
                    backup_id,
                    &plan.filesystem_payload,
                    plan.target_path.as_str(),
                    PushKind::FilesystemIntoStaging { run_id },
                )?;
                self.push_payload(
                    &session,
                    backup_id,
                    database_payload,
                    plan.target_path.as_str(),
                    PushKind::DatabaseIntoStaging { run_id },
                )?;
                self.finalize_deploy(&session, plan.target_path.as_str(), run_id)?;
            }
        }
        Ok(plan)
    }

    fn push_payload(
        &self,
        session: &SshSession,
        backup_id: &BackupId,
        payload_path: &PayloadPath,
        target_path: &str,
        kind: PushKind<'_>,
    ) -> Result<(), DeployError> {
        // Re-verifies the manifest fresh, immediately before this specific
        // payload is read — not once for the whole `execute` call. The
        // returned length is measured from the decrypted content itself,
        // never `PayloadEntry.byte_length` (which records the on-disk,
        // possibly-encrypted-and-therefore-larger stored size) — see
        // `open_deploy_payload_reader`'s own doc comment.
        let (reader, expected_bytes) = self
            .repository
            .open_deploy_payload_reader(backup_id, payload_path, self.verifier, self.credentials)
            .map_err(|_| DeployError::Storage)?;
        let identity_path = session.identity.path();
        let result = match kind {
            PushKind::FilesystemOnly => self.ssh.push_filesystem_to(
                &session.host,
                &session.user,
                identity_path,
                target_path,
                reader,
                expected_bytes,
            ),
            PushKind::FilesystemIntoStaging { run_id } => self.ssh.push_filesystem_into_staging_to(
                &session.host,
                &session.user,
                identity_path,
                StagingTarget {
                    target_path,
                    run_id,
                },
                reader,
                expected_bytes,
            ),
            PushKind::DatabaseIntoStaging { run_id } => self.ssh.push_database_into_staging_to(
                &session.host,
                &session.user,
                identity_path,
                StagingTarget {
                    target_path,
                    run_id,
                },
                reader,
                expected_bytes,
            ),
        };
        result.map(|_| ()).map_err(|_| DeployError::PushFailed)
    }

    fn finalize_deploy(
        &self,
        session: &SshSession,
        target_path: &str,
        run_id: &RunId,
    ) -> Result<(), DeployError> {
        self.ssh
            .finalize_deploy_to(
                &session.host,
                &session.user,
                session.identity.path(),
                StagingTarget {
                    target_path,
                    run_id,
                },
            )
            .map_err(|_| DeployError::PushFailed)
    }

    fn resolve_ssh_session(&self) -> Result<SshSession, DeployError> {
        let host = PinnedHost::parse(
            &self.target_profile.endpoint.host,
            self.target_profile.endpoint.port,
            &self.target_profile.endpoint.host_pin.algorithm,
            &self.target_profile.endpoint.host_pin.public_key_base64,
        )
        .map_err(|_| DeployError::InvalidTargetProfile)?;
        let user = SshUser::parse(&self.target_profile.endpoint.user)
            .map_err(|_| DeployError::InvalidTargetProfile)?;
        let identity =
            SshIdentity::from_store(self.credentials, &self.target_profile.credential_id)
                .map_err(|_| DeployError::Credential)?;
        Ok(SshSession {
            host,
            user,
            identity,
        })
    }
}

struct SshSession {
    host: PinnedHost,
    user: SshUser,
    identity: SshIdentity,
}

enum PushKind<'a> {
    FilesystemOnly,
    FilesystemIntoStaging { run_id: &'a RunId },
    DatabaseIntoStaging { run_id: &'a RunId },
}

#[derive(Debug, Error)]
pub enum DeployError {
    #[error("deploy target profile is invalid")]
    InvalidTargetProfile,
    #[error("deploy target profile does not match the expected profile")]
    TargetProfileMismatch,
    #[error("deploy target credential is unavailable")]
    Credential,
    #[error("sealed backup or repository storage is unavailable")]
    Storage,
    #[error(transparent)]
    Plan(#[from] DeploymentPlanError),
    #[error("remote preflight check failed")]
    PreflightFailed,
    #[error("deploy target path is not absent")]
    TargetNotAbsent,
    #[error("remote push failed")]
    PushFailed,
}

#[cfg(test)]
mod tests {
    use super::{DeployError, DeploymentComposition};
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use ed25519_dalek::{Signature, Signer, SigningKey, Verifier};
    use guardian_core::{
        BackupId, CredentialId, HostPin, Manifest, ManifestSigner, PayloadPath, PlanId,
        PlanReference, Producer, ProfileId, RemoteTargetPath, RepositoryId, RunId, SecretStore,
        SecretStoreError, SecretValue, SigningError, SourceIdentity, SshEndpoint, Timestamp,
        VdsProfile,
    };
    use guardian_local_repository::LocalRepository;
    use guardian_ssh::SystemOpenSsh;

    #[test]
    fn execute_rejects_a_mismatched_target_profile_id_before_touching_storage()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let repository = LocalRepository::open(root.path(), RepositoryId::parse("repo-001")?)?;
        let target = target_profile("profile-target", 1)?;
        let signer = TestSigner::new();
        let composition = DeploymentComposition {
            repository: &repository,
            ssh: &SystemOpenSsh::with_binary(root.path().join("missing-ssh")),
            target_profile: &target,
            credentials: &NoopCredentialStore,
            verifier: &signer,
        };
        let result = composition.execute(
            &RunId::parse("run-001")?,
            &ProfileId::parse("different-profile")?,
            &BackupId::parse("backup-001")?,
            RemoteTargetPath::parse("/srv/app")?,
            "irrelevant",
        );
        assert!(matches!(result, Err(DeployError::TargetProfileMismatch)));
        Ok(())
    }

    #[test]
    fn execute_rejects_the_wrong_confirmation_phrase_before_touching_ssh()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let repository = LocalRepository::open(root.path(), RepositoryId::parse("repo-002")?)?;
        let signer = TestSigner::new();
        let secrets = MemorySecrets::default();
        let backup_id = BackupId::parse("backup-002")?;
        seal_filesystem_backup(&repository, &signer, &backup_id)?;
        let target = target_profile("profile-target", 1)?;
        let composition = DeploymentComposition {
            repository: &repository,
            ssh: &SystemOpenSsh::with_binary(root.path().join("missing-ssh")),
            target_profile: &target,
            credentials: &secrets,
            verifier: &signer,
        };
        let result = composition.execute(
            &RunId::parse("run-002")?,
            &target.profile_id,
            &backup_id,
            RemoteTargetPath::parse("/srv/app")?,
            "DEPLOY the-wrong-phrase",
        );
        assert!(matches!(result, Err(DeployError::Plan(_))));
        Ok(())
    }

    #[test]
    fn execute_fails_closed_when_the_target_credential_is_missing()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let repository = LocalRepository::open(root.path(), RepositoryId::parse("repo-003")?)?;
        let signer = TestSigner::new();
        let backup_id = BackupId::parse("backup-003")?;
        seal_filesystem_backup(&repository, &signer, &backup_id)?;
        let target = target_profile("profile-target", 1)?;
        let composition = DeploymentComposition {
            repository: &repository,
            ssh: &SystemOpenSsh::with_binary(root.path().join("missing-ssh")),
            target_profile: &target,
            credentials: &NoopCredentialStore,
            verifier: &signer,
        };
        let target_path = RemoteTargetPath::parse("/srv/app")?;
        let confirmation = format!(
            "DEPLOY {} TO {}:{}",
            backup_id.as_str(),
            target.profile_id.as_str(),
            target_path.as_str()
        );
        let result = composition.execute(
            &RunId::parse("run-003")?,
            &target.profile_id,
            &backup_id,
            target_path,
            &confirmation,
        );
        assert!(matches!(result, Err(DeployError::Credential)));
        Ok(())
    }

    #[test]
    fn execute_returns_a_push_error_when_ssh_cannot_launch()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let repository = LocalRepository::open(root.path(), RepositoryId::parse("repo-004")?)?;
        let signer = TestSigner::new();
        let secrets = MemorySecrets::default();
        let backup_id = BackupId::parse("backup-004")?;
        seal_filesystem_backup(&repository, &signer, &backup_id)?;
        let target = target_profile("profile-target", 1)?;
        secrets_store_valid_key(&secrets, &target.credential_id)?;
        let composition = DeploymentComposition {
            repository: &repository,
            ssh: &SystemOpenSsh::with_binary(root.path().join("missing-ssh")),
            target_profile: &target,
            credentials: &secrets,
            verifier: &signer,
        };
        let target_path = RemoteTargetPath::parse("/srv/app")?;
        let confirmation = format!(
            "DEPLOY {} TO {}:{}",
            backup_id.as_str(),
            target.profile_id.as_str(),
            target_path.as_str()
        );
        let result = composition.execute(
            &RunId::parse("run-004")?,
            &target.profile_id,
            &backup_id,
            target_path,
            &confirmation,
        );
        assert!(matches!(result, Err(DeployError::PushFailed)));
        Ok(())
    }

    /// Confirms a combined (filesystem+database) deploy attempt still fails
    /// closed the same way a filesystem-only one does when SSH cannot
    /// launch at all. This cannot isolate "the filesystem push succeeded but
    /// the database push then failed" -- `DeploymentComposition.ssh` is a
    /// concrete `SystemOpenSsh`, and pointing it at a missing binary fails
    /// every push identically, unlike local restore's equivalent test, which
    /// could selectively deny just one payload's key. The clean-room drill
    /// (`crates/guardian-capture/tests/clean_room_drill.rs`) is the only
    /// realistic place left to prove the staged protocol's cross-payload
    /// behavior end to end.
    #[test]
    fn execute_returns_a_push_error_for_a_combined_deploy_when_ssh_cannot_launch()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let repository = LocalRepository::open(root.path(), RepositoryId::parse("repo-005")?)?;
        let signer = TestSigner::new();
        let secrets = MemorySecrets::default();
        let backup_id = BackupId::parse("backup-005")?;
        seal_combined_backup(&repository, &signer, &backup_id)?;
        let target = target_profile("profile-target", 1)?;
        secrets_store_valid_key(&secrets, &target.credential_id)?;
        let composition = DeploymentComposition {
            repository: &repository,
            ssh: &SystemOpenSsh::with_binary(root.path().join("missing-ssh")),
            target_profile: &target,
            credentials: &secrets,
            verifier: &signer,
        };
        let target_path = RemoteTargetPath::parse("/srv/app")?;
        let confirmation = format!(
            "DEPLOY {} TO {}:{}",
            backup_id.as_str(),
            target.profile_id.as_str(),
            target_path.as_str()
        );
        let result = composition.execute(
            &RunId::parse("run-005")?,
            &target.profile_id,
            &backup_id,
            target_path,
            &confirmation,
        );
        assert!(matches!(result, Err(DeployError::PushFailed)));
        Ok(())
    }

    fn seal_combined_backup(
        repository: &LocalRepository,
        signer: &TestSigner,
        backup_id: &BackupId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let run = RunId::parse(format!("run-{}", backup_id.as_str()))?;
        let staging = repository.begin_staging(run.clone())?;
        let filesystem_path = PayloadPath::parse("payload/filesystem.tar.zst")?;
        let filesystem_payload = staging.write_payload(
            "filesystem",
            filesystem_path,
            "application/zstd",
            b"payload-bytes",
        )?;
        let database_path = PayloadPath::parse("payload/database.sqlite.zst")?;
        let database_payload = staging.write_payload(
            "database",
            database_path,
            "application/vnd.sqlite3+zstd",
            b"database-bytes",
        )?;
        let mut manifest = Manifest::new(
            backup_id.clone(),
            run,
            Timestamp::parse("2026-07-15T19:00:00Z")?,
            Producer {
                name: "VDS Guardian test source".to_owned(),
                version: "0.1.0".to_owned(),
                platform: "test".to_owned(),
            },
            SourceIdentity {
                profile_id: ProfileId::parse("profile-source")?,
                host_key_fingerprint: "SHA256:source-fixture".to_owned(),
            },
            PlanReference {
                plan_id: PlanId::parse("plan-test")?,
                version: 1,
                sha256: "a".repeat(64),
            },
        );
        manifest.add_payload(filesystem_payload)?;
        manifest.add_payload(database_payload)?;
        staging.seal(manifest, Timestamp::parse("2026-07-15T19:00:01Z")?, signer)?;
        Ok(())
    }

    fn seal_filesystem_backup(
        repository: &LocalRepository,
        signer: &TestSigner,
        backup_id: &BackupId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let run = RunId::parse(format!("run-{}", backup_id.as_str()))?;
        let staging = repository.begin_staging(run.clone())?;
        let path = PayloadPath::parse("payload/filesystem.tar.zst")?;
        let payload =
            staging.write_payload("filesystem", path, "application/zstd", b"payload-bytes")?;
        let mut manifest = Manifest::new(
            backup_id.clone(),
            run,
            Timestamp::parse("2026-07-15T19:00:00Z")?,
            Producer {
                name: "VDS Guardian test source".to_owned(),
                version: "0.1.0".to_owned(),
                platform: "test".to_owned(),
            },
            SourceIdentity {
                profile_id: ProfileId::parse("profile-source")?,
                host_key_fingerprint: "SHA256:source-fixture".to_owned(),
            },
            PlanReference {
                plan_id: PlanId::parse("plan-test")?,
                version: 1,
                sha256: "a".repeat(64),
            },
        );
        manifest.add_payload(payload)?;
        staging.seal(manifest, Timestamp::parse("2026-07-15T19:00:01Z")?, signer)?;
        Ok(())
    }

    fn secrets_store_valid_key(
        secrets: &MemorySecrets,
        credential_id: &CredentialId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut payload = b"openssh-key-v1\0".to_vec();
        for value in [b"none".as_slice(), b"none", b""] {
            payload.extend_from_slice(&(value.len() as u32).to_be_bytes());
            payload.extend_from_slice(value);
        }
        let encoded = STANDARD.encode(payload);
        let key = format!(
            "-----BEGIN OPENSSH PRIVATE KEY-----\n{encoded}\n-----END OPENSSH PRIVATE KEY-----\n"
        )
        .into_bytes();
        secrets.store(credential_id, &SecretValue::new(key))?;
        Ok(())
    }

    fn target_profile(
        profile_id: &str,
        marker: u8,
    ) -> Result<VdsProfile, Box<dyn std::error::Error>> {
        Ok(VdsProfile {
            profile_id: ProfileId::parse(profile_id)?,
            label: "Target VDS".to_owned(),
            credential_id: CredentialId::parse("credential-target")?,
            endpoint: SshEndpoint {
                host: "target.example".to_owned(),
                port: 22,
                user: "backup".to_owned(),
                host_pin: pin(marker)?,
            },
        })
    }

    fn pin(marker: u8) -> Result<HostPin, Box<dyn std::error::Error>> {
        let mut blob = Vec::new();
        blob.extend_from_slice(&11_u32.to_be_bytes());
        blob.extend_from_slice(b"ssh-ed25519");
        blob.extend_from_slice(&[marker]);
        Ok(HostPin::parse("ssh-ed25519", STANDARD.encode(blob))?)
    }

    struct TestSigner {
        key: SigningKey,
    }

    impl TestSigner {
        fn new() -> Self {
            Self {
                key: SigningKey::from_bytes(&[7_u8; 32]),
            }
        }
    }

    impl ManifestSigner for TestSigner {
        fn algorithm(&self) -> &'static str {
            "Ed25519"
        }

        fn key_id(&self) -> &str {
            "test-ed25519-key"
        }

        fn sign(&self, message: &[u8]) -> Result<Vec<u8>, SigningError> {
            Ok(self.key.sign(message).to_bytes().to_vec())
        }

        fn verify(&self, message: &[u8], signature: &[u8]) -> Result<(), SigningError> {
            let signature =
                Signature::from_slice(signature).map_err(|_| SigningError::VerificationFailed)?;
            self.key
                .verifying_key()
                .verify(message, &signature)
                .map_err(|_| SigningError::VerificationFailed)
        }
    }

    struct NoopCredentialStore;

    impl SecretStore for NoopCredentialStore {
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
    struct MemorySecrets(std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>);

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
}
