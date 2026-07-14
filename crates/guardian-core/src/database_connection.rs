use crate::{CredentialId, DatabaseEngine, DatabaseId, DatabaseVersion};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DatabaseConnection {
    pub database_id: DatabaseId,
    pub engine: DatabaseEngine,
    pub host: String,
    pub port: u16,
    pub database_name: String,
    pub authentication: DatabaseAuthentication,
}

impl DatabaseConnection {
    pub fn validate(&self) -> Result<(), DatabaseConnectionError> {
        let host_valid = !self.host.is_empty()
            && self.host.len() <= 253
            && !self.host.starts_with(['-', '.'])
            && !self.host.ends_with(['-', '.'])
            && !self.host.contains("..")
            && self
                .host
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'));
        let database_valid = !self.database_name.is_empty()
            && self.database_name.len() <= 64
            && self
                .database_name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
        let authentication_valid = match &self.authentication {
            DatabaseAuthentication::SshPeer => {
                matches!(self.host.as_str(), "localhost" | "127.0.0.1")
            }
            DatabaseAuthentication::CredentialReference { credential_id } => {
                !credential_id.as_str().is_empty()
            }
        };
        (host_valid && database_valid && authentication_valid && self.port != 0)
            .then_some(())
            .ok_or(DatabaseConnectionError::Invalid)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase", tag = "mode")]
pub enum DatabaseAuthentication {
    SshPeer,
    CredentialReference { credential_id: CredentialId },
}

pub trait DatabaseServerVersionProbePort: Send + Sync {
    fn probe_server(
        &self,
        connection: &DatabaseConnection,
    ) -> Result<DatabaseVersion, DatabaseServerVersionProbeError>;
}

pub struct VerifyDatabaseConnectionUseCase<'a> {
    pub probe: &'a dyn DatabaseServerVersionProbePort,
}

impl VerifyDatabaseConnectionUseCase<'_> {
    pub fn execute(
        &self,
        connection: &DatabaseConnection,
    ) -> Result<DatabaseVersion, VerifyDatabaseConnectionError> {
        connection
            .validate()
            .map_err(VerifyDatabaseConnectionError::Connection)?;
        self.probe
            .probe_server(connection)
            .map_err(VerifyDatabaseConnectionError::Probe)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DatabaseConnectionError {
    #[error("database connection metadata is invalid")]
    Invalid,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DatabaseServerVersionProbeError {
    #[error("database server version probe is unavailable")]
    Unavailable,
    #[error("database server version probe rejected untrusted output")]
    Rejected,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VerifyDatabaseConnectionError {
    #[error("database connection is invalid")]
    Connection(#[source] DatabaseConnectionError),
    #[error("database server version probe failed")]
    Probe(#[source] DatabaseServerVersionProbeError),
}
