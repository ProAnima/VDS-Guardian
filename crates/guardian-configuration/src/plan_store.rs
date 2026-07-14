use crate::{ConfigurationStoreError, storage};
use guardian_core::{FilesystemCapturePlan, PlanId};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredCapturePlan {
    pub plan: FilesystemCapturePlan,
    pub sha256: String,
}

impl StoredCapturePlan {
    pub fn new(plan: FilesystemCapturePlan) -> Result<Self, ConfigurationStoreError> {
        let sha256 = plan
            .canonical_sha256()
            .map_err(|_| ConfigurationStoreError::Invalid)?;
        Ok(Self { plan, sha256 })
    }
    pub fn validate(&self) -> Result<(), ConfigurationStoreError> {
        let sha256 = self
            .plan
            .canonical_sha256()
            .map_err(|_| ConfigurationStoreError::Invalid)?;
        (sha256 == self.sha256)
            .then_some(())
            .ok_or(ConfigurationStoreError::Invalid)
    }
}

pub struct CapturePlanStore {
    path: PathBuf,
}

impl CapturePlanStore {
    #[must_use]
    pub fn at(directory: impl AsRef<Path>) -> Self {
        Self {
            path: directory.as_ref().join("plans.json"),
        }
    }
    pub fn list(&self) -> Result<Vec<StoredCapturePlan>, ConfigurationStoreError> {
        let _lock = storage::lock(&self.path)?;
        Ok(self.read()?.plans.into_values().collect())
    }
    pub fn upsert(&self, plan: StoredCapturePlan) -> Result<(), ConfigurationStoreError> {
        plan.validate()?;
        let _lock = storage::lock(&self.path)?;
        let mut document = self.read()?;
        document
            .plans
            .insert(plan.plan.plan_id.as_str().to_owned(), plan);
        storage::write_json(&self.path, &document)
    }
    fn read(&self) -> Result<Document, ConfigurationStoreError> {
        let document = storage::read_json(&self.path)?.unwrap_or_else(Document::empty);
        if document.format_version != FORMAT_VERSION {
            return Err(ConfigurationStoreError::Invalid);
        }
        for (id, plan) in &document.plans {
            if PlanId::parse(id).is_err()
                || plan.plan.plan_id.as_str() != id
                || plan.validate().is_err()
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
    plans: BTreeMap<String, StoredCapturePlan>,
    #[serde(flatten, default)]
    extensions: BTreeMap<String, serde_json::Value>,
}
impl Document {
    fn empty() -> Self {
        Self {
            format_version: FORMAT_VERSION,
            plans: BTreeMap::new(),
            extensions: BTreeMap::new(),
        }
    }
}
