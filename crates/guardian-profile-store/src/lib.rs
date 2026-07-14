use guardian_core::{ProfileId, VdsProfile};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
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
        Ok(self.read()?.profiles.into_values().collect())
    }

    pub fn upsert(&self, profile: VdsProfile) -> Result<(), ProfileStoreError> {
        profile
            .validate()
            .map_err(|_| ProfileStoreError::InvalidProfile)?;
        let mut document = self.read()?;
        document
            .profiles
            .insert(profile.profile_id.as_str().to_owned(), profile);
        self.write(&document)
    }

    fn read(&self) -> Result<Document, ProfileStoreError> {
        if !self.path.exists() {
            return Ok(Document::empty());
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
        let temporary = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(document).map_err(|_| ProfileStoreError::InvalidDocument)?;
        fs::write(&temporary, bytes).map_err(|_| ProfileStoreError::Io)?;
        fs::rename(temporary, &self.path).map_err(|_| ProfileStoreError::Io)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Document {
    format_version: u32,
    profiles: BTreeMap<String, VdsProfile>,
}
impl Document {
    fn empty() -> Self {
        Self {
            format_version: FORMAT_VERSION,
            profiles: BTreeMap::new(),
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
}
