use crate::RepositoryError;
use crate::filesystem::{atomic_write, ensure_directory, restrict_to_owner, sync_parent};
use crate::inventory::{TrustedBackup, load_verified_manifest, trusted_inventory};
use crate::process_lock::ProcessLock;
use crate::staging::{StagingBackup, associated_data, recovery_wrap_associated_data};
use fs2::FileExt;
use guardian_archive::{ArchiveLimits, decompress_zstd_file, extract_tar_zstd};
use guardian_core::{
    BackupId, CredentialId, ManifestVerifier, PayloadPath, RepositoryId, RestorePlan, RunId,
    SecretStore,
};
use guardian_encryption::{PayloadKey, decrypt_reader_to, decrypt_self_describing_reader_to};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

mod audit;
mod credential;
mod recovery;
mod restore;

pub(crate) use credential::random_credential_id;

const REPOSITORY_FORMAT_VERSION: u32 = 1;
const RESTORE_SCRATCH_DIR_NAME: &str = "restore";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryVerificationKey {
    pub algorithm: String,
    pub key_id: String,
    pub public_key_base64: String,
}

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
            let metadata = self.read_metadata()?;
            if metadata.format_version != REPOSITORY_FORMAT_VERSION
                || metadata.repository_id != self.id
            {
                return Err(RepositoryError::IncompatibleMetadata);
            }
            return Ok(());
        }
        self.write_metadata(&RepositoryMetadata {
            format_version: REPOSITORY_FORMAT_VERSION,
            repository_id: self.id.clone(),
            recovery_credential_id: None,
            trusted_verification_key: None,
        })
    }

    fn read_metadata(&self) -> Result<RepositoryMetadata, RepositoryError> {
        let path = self.root.join("repository.json");
        let file_type = fs::symlink_metadata(&path)
            .map_err(|source| RepositoryError::io("inspect repository metadata", source))?
            .file_type();
        if !file_type.is_file() || file_type.is_symlink() {
            return Err(RepositoryError::UnsafeFilesystemEntry);
        }
        let bytes = fs::read(&path)
            .map_err(|source| RepositoryError::io("read repository metadata", source))?;
        serde_json::from_slice(&bytes).map_err(|_| RepositoryError::IncompatibleMetadata)
    }

    fn write_metadata(&self, metadata: &RepositoryMetadata) -> Result<(), RepositoryError> {
        let bytes = serde_json::to_vec(metadata).map_err(|_| RepositoryError::Serialization)?;
        atomic_write(&self.root.join("repository.json"), &bytes)
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
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryMetadata {
    format_version: u32,
    repository_id: RepositoryId,
    /// Public reference to the repository's recovery key (ADR 0013) in
    /// whichever `SecretStore` configured it — never the key itself.
    /// Absent on a repository that never ran `recovery init`.
    #[serde(default)]
    recovery_credential_id: Option<CredentialId>,
    #[serde(default)]
    trusted_verification_key: Option<RepositoryVerificationKey>,
}

pub(crate) struct RepositoryLock {
    _file: File,
    _process_lock: ProcessLock,
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
