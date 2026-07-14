use crate::IdentityError;
use fs2::FileExt;
use serde::de::DeserializeOwned;
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static HELD_CONFIGURATIONS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

pub(crate) struct ProcessLock {
    path: PathBuf,
}

pub(crate) struct ConfigurationLock {
    _file: File,
    _process_lock: ProcessLock,
}

pub(crate) fn acquire_lock(root: &Path) -> Result<ConfigurationLock, IdentityError> {
    let process_lock = ProcessLock::acquire(root)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("signing.lock"))
        .map_err(|source| IdentityError::io("open signing configuration lock", source))?;
    match FileExt::try_lock_exclusive(&file) {
        Ok(()) => Ok(ConfigurationLock {
            _file: file,
            _process_lock: process_lock,
        }),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Err(IdentityError::Busy),
        Err(source) => Err(IdentityError::io("lock signing configuration", source)),
    }
}

impl ProcessLock {
    pub fn acquire(path: &Path) -> Result<Self, IdentityError> {
        let mut held = registry().lock().map_err(|_| IdentityError::Busy)?;
        if !held.insert(path.to_path_buf()) {
            return Err(IdentityError::Busy);
        }
        Ok(Self {
            path: path.to_path_buf(),
        })
    }
}

impl Drop for ProcessLock {
    fn drop(&mut self) {
        if let Ok(mut held) = registry().lock() {
            held.remove(&self.path);
        }
    }
}

fn registry() -> &'static Mutex<HashSet<PathBuf>> {
    HELD_CONFIGURATIONS.get_or_init(|| Mutex::new(HashSet::new()))
}

pub(crate) fn ensure_directory(path: &Path) -> Result<(), IdentityError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| IdentityError::io("inspect signing configuration directory", source))?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        Ok(())
    } else {
        Err(IdentityError::UnsafeFilesystemEntry)
    }
}

pub(crate) fn read_optional<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, IdentityError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(IdentityError::io("inspect signing metadata", source)),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(IdentityError::UnsafeFilesystemEntry);
    }
    let bytes =
        fs::read(path).map_err(|source| IdentityError::io("read signing metadata", source))?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| IdentityError::IncompatibleConfiguration)
}

pub(crate) fn atomic_write<T: serde::Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), IdentityError> {
    let bytes = serde_json::to_vec(value).map_err(|_| IdentityError::Serialization)?;
    let name = path
        .file_name()
        .ok_or(IdentityError::UnsafeFilesystemEntry)?;
    let temporary = path.with_file_name(format!("{}.tmp", name.to_string_lossy()));
    remove_regular(&temporary)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|source| IdentityError::io("create signing metadata temporary", source))?;
    file.write_all(&bytes)
        .map_err(|source| IdentityError::io("write signing metadata temporary", source))?;
    file.sync_all()
        .map_err(|source| IdentityError::io("sync signing metadata temporary", source))?;
    fs::rename(&temporary, path)
        .map_err(|source| IdentityError::io("publish signing metadata", source))?;
    sync_parent(path)
}

pub(crate) fn remove_regular(path: &Path) -> Result<(), IdentityError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => return Err(IdentityError::io("inspect signing metadata file", source)),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(IdentityError::UnsafeFilesystemEntry);
    }
    fs::remove_file(path)
        .map_err(|source| IdentityError::io("remove signing metadata file", source))?;
    sync_parent(path)
}

pub(crate) fn sync_parent(_path: &Path) -> Result<(), IdentityError> {
    #[cfg(unix)]
    {
        let parent = _path.parent().ok_or(IdentityError::UnsafeFilesystemEntry)?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|source| IdentityError::io("sync signing metadata directory", source))?;
    }
    Ok(())
}
