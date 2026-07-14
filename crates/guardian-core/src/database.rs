use crate::{ProfileId, ProfileStorePort, ProfileStorePortError, VdsProfile};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub trait DatabaseCapabilityProbePort: Send + Sync {
    fn probe(
        &self,
        profile: &VdsProfile,
    ) -> Result<Vec<DatabaseCapability>, DatabaseCapabilityProbeError>;
}

pub struct DatabasePreflightUseCase<'a> {
    pub profiles: &'a dyn ProfileStorePort,
    pub probe: &'a dyn DatabaseCapabilityProbePort,
}

impl DatabasePreflightUseCase<'_> {
    pub fn execute(
        &self,
        profile_id: &ProfileId,
    ) -> Result<Vec<DatabaseCapability>, DatabasePreflightError> {
        let profile = self
            .profiles
            .get(profile_id)
            .map_err(DatabasePreflightError::ProfileStore)?
            .ok_or(DatabasePreflightError::ProfileNotFound)?;
        let capabilities = self
            .probe
            .probe(&profile)
            .map_err(DatabasePreflightError::Probe)?;
        if capabilities.is_empty() {
            return Err(DatabasePreflightError::NoCapabilities);
        }
        for capability in &capabilities {
            capability.validate()?;
            if !capability.is_compatible() {
                return Err(DatabasePreflightError::IncompatibleVersion);
            }
        }
        Ok(capabilities)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DatabaseCapability {
    pub engine: DatabaseEngine,
    pub server_version: DatabaseVersion,
    pub dump_tool_version: DatabaseVersion,
}

impl DatabaseCapability {
    #[must_use]
    pub fn is_compatible(&self) -> bool {
        self.server_version.major == self.dump_tool_version.major
    }

    fn validate(&self) -> Result<(), DatabasePreflightError> {
        (self.server_version.is_valid() && self.dump_tool_version.is_valid())
            .then_some(())
            .ok_or(DatabasePreflightError::InvalidVersion)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DatabaseEngine {
    PostgreSql,
    MySql,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DatabaseVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl DatabaseVersion {
    pub fn new(major: u16, minor: u16, patch: u16) -> Result<Self, DatabasePreflightError> {
        (major > 0)
            .then_some(Self {
                major,
                minor,
                patch,
            })
            .ok_or(DatabasePreflightError::InvalidVersion)
    }

    fn is_valid(&self) -> bool {
        self.major > 0
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DatabaseCapabilityProbeError {
    #[error("database capability probe is unavailable")]
    Unavailable,
    #[error("database capability probe rejected untrusted output")]
    Rejected,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DatabasePreflightError {
    #[error("profile storage failed")]
    ProfileStore(#[source] ProfileStorePortError),
    #[error("profile was not found")]
    ProfileNotFound,
    #[error("database capability probe failed")]
    Probe(#[source] DatabaseCapabilityProbeError),
    #[error("database server and dump tool major versions are incompatible")]
    IncompatibleVersion,
    #[error("no supported database dump capability was discovered")]
    NoCapabilities,
    #[error("database version is invalid")]
    InvalidVersion,
}
