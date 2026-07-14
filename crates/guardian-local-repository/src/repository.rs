use crate::RepositoryError;
use crate::filesystem::{atomic_write, ensure_directory, sync_parent, write_new};
use crate::process_lock::ProcessLock;
use crate::staging::StagingBackup;
use fs2::FileExt;
use guardian_core::{BackupId, RepositoryId, RunId};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const REPOSITORY_FORMAT_VERSION: u32 = 1;
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
            if metadata.is_dir() && is_old_enough(&metadata, minimum_age)? {
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| RepositoryError::UnsafeFilesystemEntry)?;
                let run_id =
                    RunId::parse(name).map_err(|_| RepositoryError::UnsafeFilesystemEntry)?;
                self.quarantine(&run_id, "abandoned")?;
                recovered += 1;
            }
        }
        Ok(recovered)
    }

    fn ensure_layout(&self) -> Result<(), RepositoryError> {
        ensure_directory(&self.root)?;
        for name in ["staging", "backups", "quarantine", "audit"] {
            let path = self.root.join(name);
            match fs::create_dir(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    ensure_directory(&path)?;
                }
                Err(source) => {
                    return Err(RepositoryError::io("create repository directory", source));
                }
            }
        }
        Ok(())
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
struct QuarantineRecord {
    reason: &'static str,
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
