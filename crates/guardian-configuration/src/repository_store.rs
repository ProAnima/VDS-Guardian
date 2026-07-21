use crate::{ConfigurationStoreError, storage};
use guardian_core::RepositoryId;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryRegistration {
    pub repository_id: RepositoryId,
    pub label: String,
    pub path: PathBuf,
}

impl RepositoryRegistration {
    pub fn new(
        repository_id: RepositoryId,
        label: String,
        path: PathBuf,
    ) -> Result<Self, ConfigurationStoreError> {
        let registration = Self {
            repository_id,
            label,
            path,
        };
        registration.validate()?;
        Ok(registration)
    }

    pub fn validate(&self) -> Result<(), ConfigurationStoreError> {
        let label_valid = !self.label.is_empty()
            && self.label.len() <= 128
            && !self.label.chars().any(char::is_control);
        if !label_valid || !self.path.is_absolute() {
            return Err(ConfigurationStoreError::Invalid);
        }
        let metadata =
            fs::symlink_metadata(&self.path).map_err(|_| ConfigurationStoreError::Invalid)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(ConfigurationStoreError::Invalid);
        }
        let canonical =
            fs::canonicalize(&self.path).map_err(|_| ConfigurationStoreError::Unavailable)?;
        (canonical == self.path)
            .then_some(())
            .ok_or(ConfigurationStoreError::Invalid)
    }
}

pub struct RepositoryStore {
    path: PathBuf,
}

impl RepositoryStore {
    #[must_use]
    pub fn at(directory: impl AsRef<Path>) -> Self {
        Self {
            path: directory.as_ref().join("repositories.json"),
        }
    }

    pub fn list(&self) -> Result<Vec<RepositoryRegistration>, ConfigurationStoreError> {
        let _lock = storage::lock(&self.path)?;
        Ok(self.read()?.repositories.into_values().collect())
    }

    pub fn get(
        &self,
        id: &RepositoryId,
    ) -> Result<Option<RepositoryRegistration>, ConfigurationStoreError> {
        Ok(self
            .list()?
            .into_iter()
            .find(|entry| entry.repository_id == *id))
    }

    pub fn upsert(
        &self,
        registration: RepositoryRegistration,
    ) -> Result<(), ConfigurationStoreError> {
        registration.validate()?;
        let _lock = storage::lock(&self.path)?;
        let mut document = self.read()?;
        document
            .repositories
            .insert(registration.repository_id.as_str().to_owned(), registration);
        storage::write_json(&self.path, &document)
    }

    pub fn remove(
        &self,
        id: &RepositoryId,
    ) -> Result<Option<RepositoryRegistration>, ConfigurationStoreError> {
        let _lock = storage::lock(&self.path)?;
        let mut document = self.read()?;
        let removed = document.repositories.remove(id.as_str());
        if removed.is_some() {
            storage::write_json(&self.path, &document)?;
        }
        Ok(removed)
    }

    pub fn update_path(
        &self,
        id: &RepositoryId,
        path: PathBuf,
    ) -> Result<Option<RepositoryRegistration>, ConfigurationStoreError> {
        let _lock = storage::lock(&self.path)?;
        let mut document = self.read()?;
        let Some(current) = document.repositories.get(id.as_str()) else {
            return Ok(None);
        };
        let updated = RepositoryRegistration::new(id.clone(), current.label.clone(), path)?;
        document
            .repositories
            .insert(id.as_str().to_owned(), updated.clone());
        storage::write_json(&self.path, &document)?;
        Ok(Some(updated))
    }

    fn read(&self) -> Result<Document, ConfigurationStoreError> {
        let document = storage::read_json(&self.path)?.unwrap_or_else(Document::empty);
        if document.format_version != FORMAT_VERSION {
            return Err(ConfigurationStoreError::Invalid);
        }
        for (id, registration) in &document.repositories {
            if RepositoryId::parse(id).is_err()
                || registration.repository_id.as_str() != id
                || registration.validate().is_err()
            {
                return Err(ConfigurationStoreError::Invalid);
            }
        }
        Ok(document)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    format_version: u32,
    repositories: BTreeMap<String, RepositoryRegistration>,
    #[serde(flatten, default)]
    extensions: BTreeMap<String, serde_json::Value>,
}

impl Document {
    fn empty() -> Self {
        Self {
            format_version: FORMAT_VERSION,
            repositories: BTreeMap::new(),
            extensions: BTreeMap::new(),
        }
    }
}
