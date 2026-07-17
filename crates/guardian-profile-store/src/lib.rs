use fs2::FileExt;
use guardian_core::{
    ProfileId, ProfileStorePort, ProfileStorePortError, SecretStore, SecretStoreError, VdsProfile,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use thiserror::Error;

const FORMAT_VERSION: u32 = 1;

pub struct ProfileStore {
    path: PathBuf,
}

impl ProfileStore {
    #[must_use]
    pub fn at(directory: impl AsRef<Path>) -> Self {
        Self {
            path: directory.as_ref().join("profiles.json"),
        }
    }

    pub fn list(&self) -> Result<Vec<VdsProfile>, ProfileStoreError> {
        let _lock = self.lock()?;
        Ok(self.read()?.profiles.into_values().collect())
    }

    pub fn upsert(&self, profile: VdsProfile) -> Result<(), ProfileStoreError> {
        profile
            .validate()
            .map_err(|_| ProfileStoreError::InvalidProfile)?;
        let _lock = self.lock()?;
        let mut document = self.read()?;
        document
            .profiles
            .insert(profile.profile_id.as_str().to_owned(), profile);
        self.write(&document)
    }

    pub fn remove(&self, profile_id: &ProfileId) -> Result<Option<VdsProfile>, ProfileStoreError> {
        let _lock = self.lock()?;
        let mut document = self.read()?;
        let removed = document.profiles.remove(profile_id.as_str());
        if removed.is_some() {
            self.write(&document)?;
        }
        Ok(removed)
    }

    pub fn remove_with_secret(
        &self,
        profile_id: &ProfileId,
        secrets: &dyn SecretStore,
    ) -> Result<bool, ProfileDeletionError> {
        let Some(profile) = self.remove(profile_id)? else {
            return Ok(false);
        };
        if let Err(error) = secrets.delete(&profile.credential_id) {
            return match self.upsert(profile) {
                Ok(()) => Err(ProfileDeletionError::Secret(error)),
                Err(_) => Err(ProfileDeletionError::Rollback),
            };
        }
        Ok(true)
    }

    fn read(&self) -> Result<Document, ProfileStoreError> {
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Document::empty());
            }
            Err(_) => return Err(ProfileStoreError::Io),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(ProfileStoreError::UnsafeFilesystemEntry);
        }
        let bytes = fs::read(&self.path).map_err(|_| ProfileStoreError::Io)?;
        let document: Document =
            serde_json::from_slice(&bytes).map_err(|_| ProfileStoreError::InvalidDocument)?;
        if document.format_version != FORMAT_VERSION {
            return Err(ProfileStoreError::IncompatibleVersion);
        }
        for (id, profile) in &document.profiles {
            if ProfileId::parse(id).is_err()
                || profile.profile_id.as_str() != id
                || profile.validate().is_err()
            {
                return Err(ProfileStoreError::InvalidDocument);
            }
        }
        Ok(document)
    }

    fn write(&self, document: &Document) -> Result<(), ProfileStoreError> {
        let parent = self.path.parent().ok_or(ProfileStoreError::Io)?;
        fs::create_dir_all(parent).map_err(|_| ProfileStoreError::Io)?;
        let parent_metadata = fs::symlink_metadata(parent).map_err(|_| ProfileStoreError::Io)?;
        if !parent_metadata.is_dir() || parent_metadata.file_type().is_symlink() {
            return Err(ProfileStoreError::UnsafeFilesystemEntry);
        }
        let temporary = self.path.with_extension("json.tmp");
        remove_regular(&temporary)?;
        let bytes = serde_json::to_vec(document).map_err(|_| ProfileStoreError::InvalidDocument)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| ProfileStoreError::Io)?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| ProfileStoreError::Io)?;
        fs::rename(&temporary, &self.path).map_err(|_| ProfileStoreError::Io)?;
        sync_parent(&self.path)
    }

    fn lock(&self) -> Result<File, ProfileStoreError> {
        let parent = self.path.parent().ok_or(ProfileStoreError::Io)?;
        fs::create_dir_all(parent).map_err(|_| ProfileStoreError::Io)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(parent.join("profiles.lock"))
            .map_err(|_| ProfileStoreError::Io)?;
        file.lock_exclusive().map_err(|_| ProfileStoreError::Busy)?;
        Ok(file)
    }
}

impl ProfileStorePort for ProfileStore {
    fn save(&self, profile: VdsProfile) -> Result<(), ProfileStorePortError> {
        self.upsert(profile).map_err(map_port_error)
    }

    fn get(&self, profile_id: &ProfileId) -> Result<Option<VdsProfile>, ProfileStorePortError> {
        Ok(self
            .list()
            .map_err(map_port_error)?
            .into_iter()
            .find(|profile| profile.profile_id == *profile_id))
    }
}

fn map_port_error(error: ProfileStoreError) -> ProfileStorePortError {
    match error {
        ProfileStoreError::InvalidProfile
        | ProfileStoreError::InvalidDocument
        | ProfileStoreError::UnsafeFilesystemEntry => ProfileStorePortError::Rejected,
        _ => ProfileStorePortError::Unavailable,
    }
}

fn remove_regular(path: &Path) -> Result<(), ProfileStoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            fs::remove_file(path).map_err(|_| ProfileStoreError::Io)
        }
        Ok(_) => Err(ProfileStoreError::UnsafeFilesystemEntry),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ProfileStoreError::Io),
    }
}

fn sync_parent(_path: &Path) -> Result<(), ProfileStoreError> {
    #[cfg(unix)]
    File::open(_path.parent().ok_or(ProfileStoreError::Io)?)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| ProfileStoreError::Io)?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    format_version: u32,
    profiles: BTreeMap<String, VdsProfile>,
    #[serde(flatten, default)]
    extensions: BTreeMap<String, serde_json::Value>,
}
impl Document {
    fn empty() -> Self {
        Self {
            format_version: FORMAT_VERSION,
            profiles: BTreeMap::new(),
            extensions: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProfileStoreError {
    #[error("profile is invalid")]
    InvalidProfile,
    #[error("profile storage document is invalid")]
    InvalidDocument,
    #[error("profile storage version is incompatible")]
    IncompatibleVersion,
    #[error("profile storage I/O failed")]
    Io,
    #[error("profile storage is busy")]
    Busy,
    #[error("profile storage rejected an unsafe filesystem entry")]
    UnsafeFilesystemEntry,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProfileDeletionError {
    #[error(transparent)]
    Store(#[from] ProfileStoreError),
    #[error("profile credential cleanup failed")]
    Secret(#[source] SecretStoreError),
    #[error("profile deletion rollback failed")]
    Rollback,
}
