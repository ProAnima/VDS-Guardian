use crate::RepositoryError;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub(crate) fn create_safe_parent(root: &Path, relative: &str) -> Result<PathBuf, RepositoryError> {
    ensure_directory(root)?;
    let mut current = root.to_path_buf();
    let mut segments = relative.split('/').peekable();
    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            return Ok(current.join(segment));
        }
        current.push(segment);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err(RepositoryError::UnsafeFilesystemEntry),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current)
                    .map_err(|source| RepositoryError::io("create payload directory", source))?;
            }
            Err(source) => return Err(RepositoryError::io("inspect payload directory", source)),
        }
    }
    Err(RepositoryError::UnsafeFilesystemEntry)
}

pub(crate) fn ensure_directory(path: &Path) -> Result<(), RepositoryError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| RepositoryError::io("inspect repository directory", source))?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        Ok(())
    } else {
        Err(RepositoryError::UnsafeFilesystemEntry)
    }
}

pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), RepositoryError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| RepositoryError::io("create new file", source))?;
    file.write_all(bytes)
        .map_err(|source| RepositoryError::io("write new file", source))?;
    file.sync_all()
        .map_err(|source| RepositoryError::io("sync new file", source))
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), RepositoryError> {
    let file_name = path
        .file_name()
        .ok_or(RepositoryError::UnsafeFilesystemEntry)?;
    let temporary = path.with_file_name(format!("{}.tmp", file_name.to_string_lossy()));
    write_new(&temporary, bytes)?;
    fs::rename(&temporary, path)
        .map_err(|source| RepositoryError::io("publish atomic file", source))?;
    sync_parent(path)
}

pub(crate) fn sync_parent(path: &Path) -> Result<(), RepositoryError> {
    #[cfg(unix)]
    {
        let parent = path
            .parent()
            .ok_or(RepositoryError::UnsafeFilesystemEntry)?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|source| RepositoryError::io("sync parent directory", source))?;
    }
    Ok(())
}
