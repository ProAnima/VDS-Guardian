use super::*;

impl LocalRepository {
    pub fn list_sealed_backups(
        &self,
        verifier: &dyn ManifestVerifier,
    ) -> Result<Vec<TrustedBackup>, RepositoryError> {
        let _lock = self.acquire_lock()?;
        trusted_inventory(&self.backups_root(), verifier)
    }

    /// Re-verifies a sealed backup's signature and payload checksums fresh
    /// and returns its manifest. Used by deploy planning, which — unlike
    /// `plan_restore` — needs the full manifest (payload list, source
    /// identity) rather than a pre-built `RestorePlan`.
    pub fn load_verified_manifest(
        &self,
        backup_id: &BackupId,
        verifier: &dyn ManifestVerifier,
    ) -> Result<guardian_core::Manifest, RepositoryError> {
        let _lock = self.acquire_lock()?;
        load_verified_manifest(&self.backups_root().join(backup_id.as_str()), verifier)
    }

    pub fn plan_restore(
        &self,
        backup_id: &BackupId,
        destination: impl AsRef<Path>,
        verifier: &dyn ManifestVerifier,
    ) -> Result<RestorePlan, RepositoryError> {
        let destination = destination.as_ref();
        if destination.exists() {
            return Err(RepositoryError::RestoreDestinationExists);
        }
        let _lock = self.acquire_lock()?;
        let manifest =
            load_verified_manifest(&self.backups_root().join(backup_id.as_str()), verifier)?;
        RestorePlan::build(&manifest, destination).map_err(RepositoryError::RestorePlan)
    }

    /// Stages both payloads under a fresh sibling of `destination`, never
    /// touching `destination` itself until everything has succeeded — a
    /// failed second (database) payload must not leave the first
    /// (filesystem) payload's already-extracted tree sitting at a path that
    /// then blocks every future retry via `plan_restore`'s own existence
    /// guard. Mirrors `staging.rs`'s own `publish_backup`: stage, then one
    /// `fs::rename` guarded by a fresh existence check immediately before it.
    pub fn execute_restore(
        &self,
        backup_id: &BackupId,
        destination: impl AsRef<Path>,
        confirmation: &str,
        verifier: &dyn ManifestVerifier,
        secrets: &dyn SecretStore,
    ) -> Result<RestorePlan, RepositoryError> {
        let destination = destination.as_ref();
        let plan = self.plan_restore(backup_id, destination, verifier)?;
        plan.approve(confirmation)
            .map_err(RepositoryError::RestorePlan)?;
        let backup_root = self.backups_root().join(backup_id.as_str());
        let manifest = load_verified_manifest(&backup_root, verifier)?;
        // A recovery key that is configured but unavailable right now must
        // never be more fatal than "no fallback for this call" — the real
        // fail-closed check already lives in `resolve_payload_key`, which
        // still fails if the *primary* key is also missing.
        let recovery = self.load_recovery_key(secrets).ok().flatten();
        let scratch_root = self.restore_scratch_root();
        let staging = reserve_restore_staging_directory(destination)?;
        if let Err(error) = stage_restore_payloads(
            &backup_root,
            &manifest,
            &plan,
            secrets,
            recovery.as_ref(),
            &staging,
            &scratch_root,
        ) {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        if destination.exists() {
            let _ = fs::remove_dir_all(&staging);
            return Err(RepositoryError::RestoreDestinationExists);
        }
        fs::rename(&staging, destination)
            .map_err(|source| RepositoryError::io("publish restored destination", source))?;
        sync_parent(destination)?;
        Ok(plan)
    }

    /// Opens a decrypted, still-compressed reader for one payload of one
    /// sealed backup, for a remote deploy push, alongside the *measured*
    /// byte length of that decrypted content. Re-verifies the manifest's
    /// signature and checksums fresh on *every* call rather than trusting a
    /// previously-built plan — deploy's two payload pushes are network-bound
    /// and can each run for minutes, a materially larger time-of-check to
    /// time-of-use window than local restore's back-to-back extractions, so
    /// each payload gets its own fresh verification immediately before it is
    /// read.
    ///
    /// The returned length is measured from the decrypted content itself,
    /// never taken from `PayloadEntry.byte_length` — that field records the
    /// on-disk (encrypted, when the payload is encrypted) stored size, a
    /// distinct and strictly larger number checked separately by
    /// `verify_payload_tree`. A caller that needs an exact expected byte
    /// count for what this reader will actually produce (a strict push,
    /// for instance) must use this measured value, not the manifest field.
    pub fn open_deploy_payload_reader(
        &self,
        backup_id: &BackupId,
        payload_path: &PayloadPath,
        verifier: &dyn ManifestVerifier,
        secrets: &dyn SecretStore,
    ) -> Result<(impl std::io::Read + Send + use<>, u64), RepositoryError> {
        let _lock = self.acquire_lock()?;
        let backup_root = self.backups_root().join(backup_id.as_str());
        let manifest = load_verified_manifest(&backup_root, verifier)?;
        let recovery = self.load_recovery_key(secrets).ok().flatten();
        let (payload, encryption) = resolve_payload_file(&backup_root, &manifest, payload_path)?;
        let reader = decrypted_payload_reader(
            &payload,
            payload_path,
            encryption.as_ref(),
            &manifest.backup_id,
            secrets,
            recovery.as_ref(),
            &self.restore_scratch_root(),
        )?;
        let byte_length = reader.measured_len()?;
        Ok((reader, byte_length))
    }
}

fn resolve_payload_file(
    backup_root: &Path,
    manifest: &guardian_core::Manifest,
    payload_path: &PayloadPath,
) -> Result<(PathBuf, Option<guardian_core::PayloadEncryption>), RepositoryError> {
    let entry = manifest
        .payloads
        .iter()
        .find(|entry| entry.path == *payload_path)
        .ok_or(RepositoryError::IntegrityFailure)?;
    let payload = backup_root.join(entry.path.as_str());
    let metadata = fs::symlink_metadata(&payload)
        .map_err(|source| RepositoryError::io("inspect restore payload", source))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(RepositoryError::UnsafeFilesystemEntry);
    }
    Ok((payload, entry.encryption.clone()))
}

/// Claims a unique, not-yet-existing directory name as a sibling of
/// `destination` (same parent, so a later rename between them stays a
/// same-filesystem operation) and immediately frees the name back up —
/// `extract_tar_zstd` requires a destination that does not yet exist and
/// creates it itself, so the name can only be reserved, not pre-created.
/// The small local TOCTOU window this leaves (another process claiming the
/// same name before extraction starts) has direct precedent in this crate
/// (`staging.rs`'s `reserve_payload_destination`) and is backed by the same
/// kind of real guarantee: `extract_tar_zstd`'s own `fs::create_dir` fails
/// closed, never corrupts, if the name was reclaimed.
fn reserve_restore_staging_directory(destination: &Path) -> Result<PathBuf, RepositoryError> {
    let parent = destination
        .parent()
        .ok_or(RepositoryError::UnsafeFilesystemEntry)?;
    let staging = tempfile::Builder::new()
        .prefix(".guardian-restore-tmp-")
        .tempdir_in(parent)
        .map_err(|source| RepositoryError::io("reserve restore staging directory", source))?;
    let path = staging.path().to_path_buf();
    staging
        .close()
        .map_err(|source| RepositoryError::io("free restore staging directory name", source))?;
    Ok(path)
}

fn stage_restore_payloads(
    backup_root: &Path,
    manifest: &guardian_core::Manifest,
    plan: &RestorePlan,
    secrets: &dyn SecretStore,
    recovery: Option<&PayloadKey>,
    staging: &Path,
    scratch_root: &Path,
) -> Result<(), RepositoryError> {
    extract_payload(
        backup_root,
        manifest,
        &plan.filesystem_payload,
        secrets,
        recovery,
        staging,
        scratch_root,
    )?;
    if let Some(database_payload) = &plan.database_payload {
        extract_database_payload(
            backup_root,
            manifest,
            database_payload,
            secrets,
            recovery,
            staging,
            scratch_root,
        )?;
    }
    Ok(())
}

fn extract_payload(
    backup_root: &Path,
    manifest: &guardian_core::Manifest,
    payload_path: &PayloadPath,
    secrets: &dyn SecretStore,
    recovery: Option<&PayloadKey>,
    destination: &Path,
    scratch_root: &Path,
) -> Result<guardian_archive::ArchiveInspection, RepositoryError> {
    let (payload, encryption) = resolve_payload_file(backup_root, manifest, payload_path)?;
    let source = decrypted_payload_reader(
        &payload,
        payload_path,
        encryption.as_ref(),
        &manifest.backup_id,
        secrets,
        recovery,
        scratch_root,
    )?;
    extract_tar_zstd(source, destination, ArchiveLimits::conservative())
        .map_err(RepositoryError::RestoreExtraction)
}

fn extract_database_payload(
    backup_root: &Path,
    manifest: &guardian_core::Manifest,
    payload_path: &PayloadPath,
    secrets: &dyn SecretStore,
    recovery: Option<&PayloadKey>,
    destination: &Path,
    scratch_root: &Path,
) -> Result<u64, RepositoryError> {
    let (payload, encryption) = resolve_payload_file(backup_root, manifest, payload_path)?;
    let source = decrypted_payload_reader(
        &payload,
        payload_path,
        encryption.as_ref(),
        &manifest.backup_id,
        secrets,
        recovery,
        scratch_root,
    )?;
    // Bounds the *decompressed* database size; the compressed stream itself
    // was already bounded and digest-verified at capture and load time.
    decompress_zstd_file(
        source,
        &destination.join("database.sqlite"),
        ArchiveLimits::conservative().max_expanded_bytes,
    )
    .map_err(RepositoryError::RestoreExtraction)
}

/// Decrypts (when the payload is encrypted) into a hardened scratch file and
/// returns a reader over the plaintext bytes, or opens the payload directly
/// when it is not encrypted. The scratch file, when used, is kept alive for
/// as long as the returned reader is, and is removed once the reader (and
/// its guard) is dropped.
mod crypto;

use crypto::decrypted_payload_reader;
