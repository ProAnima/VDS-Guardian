use crate::RepositoryError;
use crate::filesystem::{
    atomic_write, ensure_directory, restrict_to_owner, sync_parent, write_new,
};
use crate::inventory::{TrustedBackup, load_verified_manifest, trusted_inventory};
use crate::process_lock::ProcessLock;
use crate::staging::{StagingBackup, associated_data};
use fs2::FileExt;
use guardian_archive::{ArchiveLimits, decompress_zstd_file, extract_tar_zstd};
use guardian_core::{
    BackupId, ManifestVerifier, PayloadPath, ProfileId, RepositoryId, RestorePlan, RunId,
    SecretStore,
};
use guardian_encryption::{PayloadKey, decrypt_reader_to};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const REPOSITORY_FORMAT_VERSION: u32 = 1;
const RESTORE_SCRATCH_DIR_NAME: &str = "restore";
pub struct LocalRepository {
    root: PathBuf,
    id: RepositoryId,
}

impl LocalRepository {
    pub fn open(path: impl AsRef<Path>, id: RepositoryId) -> Result<Self, RepositoryError> {
        fs::create_dir_all(path.as_ref())
            .map_err(|source| RepositoryError::io("create repository root", source))?;
        let root = fs::canonicalize(path.as_ref())
            .map_err(|source| RepositoryError::io("canonicalize repository root", source))?;
        let repository = Self { root, id };
        repository.ensure_layout()?;
        let _lock = repository.acquire_lock()?;
        repository.ensure_metadata()?;
        repository.reconcile_retention_locked()?;
        Ok(repository)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn begin_staging(&self, run_id: RunId) -> Result<StagingBackup<'_>, RepositoryError> {
        let lock = self.acquire_lock()?;
        let path = self.staging_root().join(run_id.as_str());
        match fs::create_dir(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(RepositoryError::StagingExists);
            }
            Err(source) => return Err(RepositoryError::io("create staging run", source)),
        }
        fs::create_dir(path.join("payload"))
            .map_err(|source| RepositoryError::io("create staging payload directory", source))?;
        fs::create_dir(path.join("reports"))
            .map_err(|source| RepositoryError::io("create staging reports directory", source))?;
        sync_parent(&path)?;
        Ok(StagingBackup {
            repository: self,
            run_id,
            path,
            _lock: lock,
            payload_credentials: std::sync::Mutex::new(Vec::new()),
        })
    }

    pub fn recover_abandoned_staging(
        &self,
        minimum_age: Duration,
    ) -> Result<usize, RepositoryError> {
        let _lock = self.acquire_lock()?;
        let mut recovered = 0;
        for entry in fs::read_dir(self.staging_root())
            .map_err(|source| RepositoryError::io("list staging runs", source))?
        {
            let entry =
                entry.map_err(|source| RepositoryError::io("read staging entry", source))?;
            let metadata = entry
                .metadata()
                .map_err(|source| RepositoryError::io("inspect staging entry", source))?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| RepositoryError::UnsafeFilesystemEntry)?;
            if name == RESTORE_SCRATCH_DIR_NAME {
                continue;
            }
            if metadata.is_dir() && is_old_enough(&metadata, minimum_age)? {
                let run_id =
                    RunId::parse(name).map_err(|_| RepositoryError::UnsafeFilesystemEntry)?;
                self.quarantine(&run_id, "abandoned")?;
                recovered += 1;
            }
        }
        Ok(recovered)
    }

    pub fn recover_abandoned_restores(
        &self,
        minimum_age: Duration,
    ) -> Result<usize, RepositoryError> {
        let _lock = self.acquire_lock()?;
        let mut recovered = 0;
        for entry in fs::read_dir(self.restore_scratch_root())
            .map_err(|source| RepositoryError::io("list restore scratch files", source))?
        {
            let entry = entry
                .map_err(|source| RepositoryError::io("read restore scratch entry", source))?;
            let metadata = entry
                .metadata()
                .map_err(|source| RepositoryError::io("inspect restore scratch entry", source))?;
            if metadata.is_file() && is_old_enough(&metadata, minimum_age)? {
                fs::remove_file(entry.path()).map_err(|source| {
                    RepositoryError::io("remove abandoned restore scratch file", source)
                })?;
                recovered += 1;
            }
        }
        Ok(recovered)
    }

    fn ensure_layout(&self) -> Result<(), RepositoryError> {
        ensure_directory(&self.root)?;
        for name in ["staging", "backups", "quarantine", "audit"] {
            create_or_verify_directory(&self.root.join(name))?;
        }
        create_or_verify_directory(&self.restore_scratch_root())
    }

    fn ensure_metadata(&self) -> Result<(), RepositoryError> {
        let path = self.root.join("repository.json");
        if path.exists() {
            let file_type = fs::symlink_metadata(&path)
                .map_err(|source| RepositoryError::io("inspect repository metadata", source))?
                .file_type();
            if !file_type.is_file() || file_type.is_symlink() {
                return Err(RepositoryError::UnsafeFilesystemEntry);
            }
            let bytes = fs::read(&path)
                .map_err(|source| RepositoryError::io("read repository metadata", source))?;
            let metadata: RepositoryMetadata = serde_json::from_slice(&bytes)
                .map_err(|_| RepositoryError::IncompatibleMetadata)?;
            if metadata.format_version != REPOSITORY_FORMAT_VERSION
                || metadata.repository_id != self.id
            {
                return Err(RepositoryError::IncompatibleMetadata);
            }
            return Ok(());
        }
        let metadata = RepositoryMetadata {
            format_version: REPOSITORY_FORMAT_VERSION,
            repository_id: self.id.clone(),
        };
        let bytes = serde_json::to_vec(&metadata).map_err(|_| RepositoryError::Serialization)?;
        atomic_write(&path, &bytes)
    }

    pub(crate) fn acquire_lock(&self) -> Result<RepositoryLock, RepositoryError> {
        let process_lock = ProcessLock::acquire(&self.root)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join("repository.lock"))
            .map_err(|source| RepositoryError::io("open repository lock", source))?;
        match FileExt::try_lock_exclusive(&file) {
            Ok(()) => Ok(RepositoryLock {
                _file: file,
                _process_lock: process_lock,
            }),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                Err(RepositoryError::Busy)
            }
            Err(source) => Err(RepositoryError::io("lock repository", source)),
        }
    }

    pub(crate) fn quarantine(
        &self,
        run_id: &RunId,
        reason: &'static str,
    ) -> Result<PathBuf, RepositoryError> {
        let staging = self.staging_root().join(run_id.as_str());
        ensure_directory(&staging)?;
        let record = QuarantineRecord { reason };
        let bytes = serde_json::to_vec(&record).map_err(|_| RepositoryError::Serialization)?;
        atomic_write(&staging.join("quarantine.json"), &bytes)?;
        let destination = self.quarantine_root().join(run_id.as_str());
        fs::rename(&staging, &destination)
            .map_err(|source| RepositoryError::io("quarantine staging run", source))?;
        sync_parent(&destination)?;
        Ok(destination)
    }

    fn staging_root(&self) -> PathBuf {
        self.root.join("staging")
    }

    fn restore_scratch_root(&self) -> PathBuf {
        self.staging_root().join(RESTORE_SCRATCH_DIR_NAME)
    }

    pub(crate) fn backups_root(&self) -> PathBuf {
        self.root.join("backups")
    }

    pub(crate) fn id(&self) -> &RepositoryId {
        &self.id
    }

    pub(crate) fn quarantine_root(&self) -> PathBuf {
        self.root.join("quarantine")
    }

    pub(crate) fn audit_root(&self) -> PathBuf {
        self.root.join("audit")
    }

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
        let scratch_root = self.restore_scratch_root();
        extract_payload(
            &backup_root,
            &manifest,
            &plan.filesystem_payload,
            secrets,
            destination,
            &scratch_root,
        )?;
        if let Some(database_payload) = &plan.database_payload {
            extract_database_payload(
                &backup_root,
                &manifest,
                database_payload,
                secrets,
                destination,
                &scratch_root,
            )?;
        }
        Ok(plan)
    }

    /// Opens a decrypted, still-compressed reader for one payload of one
    /// sealed backup, for a remote deploy push. Re-verifies the manifest's
    /// signature and checksums fresh on *every* call rather than trusting a
    /// previously-built plan — deploy's two payload pushes are network-bound
    /// and can each run for minutes, a materially larger time-of-check to
    /// time-of-use window than local restore's back-to-back extractions, so
    /// each payload gets its own fresh verification immediately before it is
    /// read.
    pub fn open_deploy_payload_reader(
        &self,
        backup_id: &BackupId,
        payload_path: &PayloadPath,
        verifier: &dyn ManifestVerifier,
        secrets: &dyn SecretStore,
    ) -> Result<impl std::io::Read + Send + use<>, RepositoryError> {
        let _lock = self.acquire_lock()?;
        let backup_root = self.backups_root().join(backup_id.as_str());
        let manifest = load_verified_manifest(&backup_root, verifier)?;
        let (payload, encryption) = resolve_payload_file(&backup_root, &manifest, payload_path)?;
        decrypted_payload_reader(
            &payload,
            payload_path,
            encryption.as_ref(),
            &manifest.backup_id,
            secrets,
            &self.restore_scratch_root(),
        )
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

fn extract_payload(
    backup_root: &Path,
    manifest: &guardian_core::Manifest,
    payload_path: &PayloadPath,
    secrets: &dyn SecretStore,
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
fn decrypted_payload_reader(
    payload: &Path,
    payload_path: &PayloadPath,
    encryption: Option<&guardian_core::PayloadEncryption>,
    backup_id: &BackupId,
    secrets: &dyn SecretStore,
    scratch_root: &Path,
) -> Result<DecryptedPayload, RepositoryError> {
    let Some(encryption) = encryption else {
        let file = File::open(payload)
            .map_err(|error| RepositoryError::io("open restore payload", error))?;
        return Ok(DecryptedPayload::Direct(file));
    };
    let secret = secrets
        .load(&encryption.credential_id)
        .map_err(|_| RepositoryError::Credential)?;
    let secret = secret.ok_or(RepositoryError::Credential)?;
    let key = PayloadKey::from_bytes(secret.expose()).map_err(|_| RepositoryError::Encryption)?;
    let nonce = encryption.nonce()?;
    let temporary = tempfile::NamedTempFile::new_in(scratch_root)
        .map_err(|error| RepositoryError::io("create temporary decrypted payload", error))?;
    restrict_to_owner(temporary.path())?;
    let mut encrypted = File::open(payload)
        .map_err(|error| RepositoryError::io("open encrypted restore payload", error))?;
    let mut plaintext = temporary
        .reopen()
        .map_err(|error| RepositoryError::io("open temporary decrypted payload", error))?;
    decrypt_reader_to(
        &key,
        &mut encrypted,
        &mut plaintext,
        &associated_data(backup_id, payload_path),
        &nonce,
    )
    .map_err(|_| RepositoryError::Encryption)?;
    let file = temporary
        .reopen()
        .map_err(|error| RepositoryError::io("read temporary decrypted payload", error))?;
    Ok(DecryptedPayload::Temporary {
        _guard: temporary,
        file,
    })
}

enum DecryptedPayload {
    Temporary {
        _guard: tempfile::NamedTempFile,
        file: File,
    },
    Direct(File),
}

impl std::io::Read for DecryptedPayload {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self {
            DecryptedPayload::Temporary { file, .. } => file.read(buffer),
            DecryptedPayload::Direct(file) => file.read(buffer),
        }
    }
}

pub(crate) struct RepositoryLock {
    _file: File,
    _process_lock: ProcessLock,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryMetadata {
    format_version: u32,
    repository_id: RepositoryId,
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
struct QuarantineRecord {
    reason: &'static str,
}

fn create_or_verify_directory(path: &Path) -> Result<(), RepositoryError> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => ensure_directory(path),
        Err(source) => Err(RepositoryError::io("create repository directory", source)),
    }
}

fn is_old_enough(metadata: &fs::Metadata, minimum_age: Duration) -> Result<bool, RepositoryError> {
    let modified = metadata
        .modified()
        .map_err(|source| RepositoryError::io("read staging modification time", source))?;
    let age = match SystemTime::now().duration_since(modified) {
        Ok(duration) => duration,
        Err(_) => Duration::ZERO,
    };
    Ok(age >= minimum_age)
}
