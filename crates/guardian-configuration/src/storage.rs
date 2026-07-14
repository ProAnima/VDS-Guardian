use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigurationStoreError {
    #[error("configuration is invalid")]
    Invalid,
    #[error("configuration storage is unavailable")]
    Unavailable,
}

pub(crate) fn read_json<T: serde::de::DeserializeOwned>(
    path: &Path,
) -> Result<Option<T>, ConfigurationStoreError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            serde_json::from_slice(
                &fs::read(path).map_err(|_| ConfigurationStoreError::Unavailable)?,
            )
            .map(Some)
            .map_err(|_| ConfigurationStoreError::Invalid)
        }
        _ => Err(ConfigurationStoreError::Invalid),
    }
}

pub(crate) fn write_json<T: serde::Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), ConfigurationStoreError> {
    let parent = path.parent().ok_or(ConfigurationStoreError::Unavailable)?;
    fs::create_dir_all(parent).map_err(|_| ConfigurationStoreError::Unavailable)?;
    let parent_metadata =
        fs::symlink_metadata(parent).map_err(|_| ConfigurationStoreError::Unavailable)?;
    if !parent_metadata.is_dir() || parent_metadata.file_type().is_symlink() {
        return Err(ConfigurationStoreError::Invalid);
    }
    let temporary = path.with_extension("json.tmp");
    remove_regular(&temporary)?;
    let bytes = serde_json::to_vec(value).map_err(|_| ConfigurationStoreError::Invalid)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| ConfigurationStoreError::Unavailable)?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| ConfigurationStoreError::Unavailable)?;
    fs::rename(temporary, path).map_err(|_| ConfigurationStoreError::Unavailable)?;
    Ok(())
}

pub(crate) fn lock(path: &Path) -> Result<File, ConfigurationStoreError> {
    let parent = path.parent().ok_or(ConfigurationStoreError::Unavailable)?;
    fs::create_dir_all(parent).map_err(|_| ConfigurationStoreError::Unavailable)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(parent.join("configuration.lock"))
        .map_err(|_| ConfigurationStoreError::Unavailable)?;
    file.lock_exclusive()
        .map_err(|_| ConfigurationStoreError::Unavailable)?;
    Ok(file)
}

fn remove_regular(path: &Path) -> Result<(), ConfigurationStoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            fs::remove_file(path).map_err(|_| ConfigurationStoreError::Unavailable)
        }
        Ok(_) => Err(ConfigurationStoreError::Invalid),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ConfigurationStoreError::Unavailable),
    }
}
