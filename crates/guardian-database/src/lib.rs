//! Bounded parsing and pinned-SSH execution for database dump-tool discovery.

use guardian_core::{
    DatabaseAuthentication, DatabaseCapability, DatabaseCapabilityProbeError,
    DatabaseCapabilityProbePort, DatabaseConnection, DatabaseEngine,
    DatabaseServerVersionProbeError, DatabaseServerVersionProbePort, DatabaseVersion, SecretStore,
    VdsProfile,
};
use guardian_ssh::{PinnedHost, SshIdentity, SshUser, SystemOpenSsh};
use std::fs;
use tempfile::tempdir;
use thiserror::Error;

pub const MAX_PROBE_BYTES: usize = 64 * 1024;
pub const MAX_SERVER_VERSION_BYTES: usize = 4 * 1024;

pub struct SshDumpToolProbe<'a> {
    pub ssh: &'a SystemOpenSsh,
    pub credentials: &'a dyn SecretStore,
}

impl SshDumpToolProbe<'_> {
    pub fn probe(&self, profile: &VdsProfile) -> Result<Vec<DumpToolVersion>, DumpToolProbeError> {
        profile
            .validate()
            .map_err(|_| DumpToolProbeError::Rejected)?;
        let host = PinnedHost::parse(
            &profile.endpoint.host,
            profile.endpoint.port,
            &profile.endpoint.host_pin.algorithm,
            &profile.endpoint.host_pin.public_key_base64,
        )
        .map_err(|_| DumpToolProbeError::Rejected)?;
        let user =
            SshUser::parse(&profile.endpoint.user).map_err(|_| DumpToolProbeError::Rejected)?;
        let identity = SshIdentity::from_store(self.credentials, &profile.credential_id)
            .map_err(|_| DumpToolProbeError::Unavailable)?;
        let temporary = tempdir().map_err(|_| DumpToolProbeError::Unavailable)?;
        let destination = temporary.path().join("database-tools.txt");
        self.ssh
            .probe_database_tools_to(
                &host,
                &user,
                identity.path(),
                &destination,
                u64::try_from(MAX_PROBE_BYTES).map_err(|_| DumpToolProbeError::Unavailable)?,
            )
            .map_err(|_| DumpToolProbeError::Unavailable)?;
        parse_dump_tool_probe(&fs::read(destination).map_err(|_| DumpToolProbeError::Unavailable)?)
    }
}

pub struct SshPeerServerVersionProbe<'a> {
    pub ssh: &'a SystemOpenSsh,
    pub profile: &'a VdsProfile,
    pub credentials: &'a dyn SecretStore,
}

/// Composes fixed dump-tool discovery with fixed server-version probes for one
/// pinned SSH profile. Database connection metadata remains explicit input so a
/// preflight cannot silently succeed without a server capability to compare.
pub struct SshDatabaseCapabilityProbe<'a> {
    pub ssh: &'a SystemOpenSsh,
    pub credentials: &'a dyn SecretStore,
    pub connections: &'a [DatabaseConnection],
}

impl DatabaseCapabilityProbePort for SshDatabaseCapabilityProbe<'_> {
    fn probe(
        &self,
        profile: &VdsProfile,
    ) -> Result<Vec<DatabaseCapability>, DatabaseCapabilityProbeError> {
        let tools = SshDumpToolProbe {
            ssh: self.ssh,
            credentials: self.credentials,
        }
        .probe(profile)
        .map_err(map_dump_tool_error)?;
        let server_probe = SshPeerServerVersionProbe {
            ssh: self.ssh,
            profile,
            credentials: self.credentials,
        };
        build_capabilities(self.connections, &tools, &server_probe)
    }
}

pub fn build_capabilities(
    connections: &[DatabaseConnection],
    tools: &[DumpToolVersion],
    server_probe: &dyn DatabaseServerVersionProbePort,
) -> Result<Vec<DatabaseCapability>, DatabaseCapabilityProbeError> {
    if connections.is_empty() || tools.is_empty() {
        return Err(DatabaseCapabilityProbeError::Rejected);
    }
    connections
        .iter()
        .map(|connection| {
            connection
                .validate()
                .map_err(|_| DatabaseCapabilityProbeError::Rejected)?;
            let dump_tool = tools
                .iter()
                .find(|tool| tool.engine == connection.engine)
                .ok_or(DatabaseCapabilityProbeError::Rejected)?;
            let server_version = server_probe
                .probe_server(connection)
                .map_err(map_server_version_error)?;
            Ok(DatabaseCapability {
                engine: connection.engine,
                server_version,
                dump_tool_version: dump_tool.version,
            })
        })
        .collect()
}

fn map_dump_tool_error(error: DumpToolProbeError) -> DatabaseCapabilityProbeError {
    match error {
        DumpToolProbeError::Unavailable => DatabaseCapabilityProbeError::Unavailable,
        DumpToolProbeError::Rejected => DatabaseCapabilityProbeError::Rejected,
    }
}

fn map_server_version_error(
    error: DatabaseServerVersionProbeError,
) -> DatabaseCapabilityProbeError {
    match error {
        DatabaseServerVersionProbeError::Unavailable => DatabaseCapabilityProbeError::Unavailable,
        DatabaseServerVersionProbeError::Rejected => DatabaseCapabilityProbeError::Rejected,
    }
}

impl DatabaseServerVersionProbePort for SshPeerServerVersionProbe<'_> {
    fn probe_server(
        &self,
        connection: &DatabaseConnection,
    ) -> Result<DatabaseVersion, DatabaseServerVersionProbeError> {
        if !matches!(connection.authentication, DatabaseAuthentication::SshPeer) {
            return Err(DatabaseServerVersionProbeError::Rejected);
        }
        self.profile
            .validate()
            .map_err(|_| DatabaseServerVersionProbeError::Rejected)?;
        let host = PinnedHost::parse(
            &self.profile.endpoint.host,
            self.profile.endpoint.port,
            &self.profile.endpoint.host_pin.algorithm,
            &self.profile.endpoint.host_pin.public_key_base64,
        )
        .map_err(|_| DatabaseServerVersionProbeError::Rejected)?;
        let user = SshUser::parse(&self.profile.endpoint.user)
            .map_err(|_| DatabaseServerVersionProbeError::Rejected)?;
        let identity = SshIdentity::from_store(self.credentials, &self.profile.credential_id)
            .map_err(|_| DatabaseServerVersionProbeError::Unavailable)?;
        let temporary = tempdir().map_err(|_| DatabaseServerVersionProbeError::Unavailable)?;
        let destination = temporary.path().join("database-server-version.txt");
        self.ssh
            .probe_database_server_to(
                &host,
                &user,
                identity.path(),
                connection,
                &destination,
                u64::try_from(MAX_SERVER_VERSION_BYTES)
                    .map_err(|_| DatabaseServerVersionProbeError::Unavailable)?,
            )
            .map_err(|_| DatabaseServerVersionProbeError::Unavailable)?;
        parse_server_version(
            &fs::read(destination).map_err(|_| DatabaseServerVersionProbeError::Unavailable)?,
        )
        .map_err(|_| DatabaseServerVersionProbeError::Rejected)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DumpToolVersion {
    pub engine: DatabaseEngine,
    pub version: DatabaseVersion,
}

pub fn parse_dump_tool_probe(bytes: &[u8]) -> Result<Vec<DumpToolVersion>, DumpToolProbeError> {
    if bytes.len() > MAX_PROBE_BYTES {
        return Err(DumpToolProbeError::Rejected);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| DumpToolProbeError::Rejected)?;
    let mut tools = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let (engine, output) = line.split_once('\t').ok_or(DumpToolProbeError::Rejected)?;
        let engine = match engine {
            "postgresql" => DatabaseEngine::PostgreSql,
            "mysql" => DatabaseEngine::MySql,
            _ => return Err(DumpToolProbeError::Rejected),
        };
        if tools
            .iter()
            .any(|tool: &DumpToolVersion| tool.engine == engine)
        {
            return Err(DumpToolProbeError::Rejected);
        }
        tools.push(DumpToolVersion {
            engine,
            version: parse_version(output)?,
        });
    }
    (!tools.is_empty())
        .then_some(tools)
        .ok_or(DumpToolProbeError::Rejected)
}

pub fn parse_server_version(bytes: &[u8]) -> Result<DatabaseVersion, DumpToolProbeError> {
    if bytes.len() > MAX_SERVER_VERSION_BYTES {
        return Err(DumpToolProbeError::Rejected);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| DumpToolProbeError::Rejected)?;
    let version = text.trim();
    if version.is_empty() || version.contains(['\r', '\n']) {
        return Err(DumpToolProbeError::Rejected);
    }
    parse_version(version)
}

fn parse_version(output: &str) -> Result<DatabaseVersion, DumpToolProbeError> {
    for token in output.split(|character: char| !character.is_ascii_digit() && character != '.') {
        let parts = token.split('.').collect::<Vec<_>>();
        if !(2..=3).contains(&parts.len()) || parts.iter().any(|part| part.is_empty()) {
            continue;
        }
        let values = parts
            .iter()
            .map(|part| part.parse::<u16>())
            .collect::<Result<Vec<_>, _>>();
        let Ok(values) = values else {
            continue;
        };
        if values[0] == 0 {
            continue;
        }
        return Ok(DatabaseVersion {
            major: values[0],
            minor: values[1],
            patch: *values.get(2).unwrap_or(&0),
        });
    }
    Err(DumpToolProbeError::Rejected)
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DumpToolProbeError {
    #[error("database dump-tool probe is unavailable")]
    Unavailable,
    #[error("database dump-tool probe rejected untrusted output")]
    Rejected,
}
