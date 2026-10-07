//! Yes/no checks of what the server offers (tar with zstd, zstd, sqlite3, a working login) and
//! the exact `ssh` arguments each one uses.

use crate::remote_commands::{
    database_disk_budget_probe_command, database_server_probe_command, database_tool_probe_command,
    docker_inspect_command, sqlite_snapshot_command, sqlite3_probe_command, zstd_probe_command,
};
use crate::{PinnedHost, SshError, SshIdentity, SshUser, SystemOpenSsh, map_wait_error, process};
use guardian_core::DatabaseConnection;
use std::{ffi::OsString, path::Path, process::Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteCapabilities {
    pub tar_zstd: bool,
}

impl SystemOpenSsh {
    pub fn probe_zstd(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
    ) -> Result<bool, SshError> {
        self.succeeds(host, identity, |known_hosts| {
            self.zstd_probe_arguments(host, user, identity, known_hosts)
        })
    }

    #[must_use]
    pub fn zstd_probe_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
        known_hosts: &Path,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity,
            known_hosts,
            zstd_probe_command().into(),
        )
    }

    pub fn probe_tar_zstd(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
    ) -> Result<RemoteCapabilities, SshError> {
        let tar_zstd = self.succeeds(host, identity, |known_hosts| {
            self.capability_probe_arguments(host, user, identity, known_hosts)
        })?;
        Ok(RemoteCapabilities { tar_zstd })
    }

    pub fn probe_sqlite3(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
    ) -> Result<bool, SshError> {
        self.succeeds(host, identity, |known_hosts| {
            self.sqlite3_probe_arguments(host, user, identity, known_hosts)
        })
    }

    pub fn probe_connection(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
    ) -> Result<(), SshError> {
        let connected = self.succeeds(host, identity, |known_hosts| {
            self.connection_probe_arguments(host, user, identity, known_hosts)
        })?;
        connected.then_some(()).ok_or(SshError::CaptureFailed)
    }

    /// Runs one silent remote check against the pinned host and reports whether it exited 0.
    fn succeeds(
        &self,
        host: &PinnedHost,
        identity: &SshIdentity,
        arguments: impl FnOnce(&Path) -> Vec<OsString>,
    ) -> Result<bool, SshError> {
        let known_hosts = self.known_hosts_file(host)?;
        // Holds the password broker (if any) until the child has finished.
        let mut ssh_command = self.new_command(identity)?;
        let child = ssh_command
            .args(arguments(known_hosts.as_ref()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| SshError::LaunchFailed)?;
        let status = process::wait_for_exit(child, self.total_timeout, &self.cancellation)
            .map_err(map_wait_error)?;
        Ok(status.success())
    }

    #[must_use]
    pub fn capability_probe_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
        known_hosts: &Path,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity,
            known_hosts,
            "LC_ALL=C tar --create --zstd --file=/dev/null --files-from=/dev/null >/dev/null 2>&1"
                .into(),
        )
    }

    #[must_use]
    pub fn connection_probe_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
        known_hosts: &Path,
    ) -> Vec<OsString> {
        self.arguments_for_command(host, user, identity, known_hosts, "true".into())
    }

    #[must_use]
    pub fn docker_inspect_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
        known_hosts: &Path,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity,
            known_hosts,
            docker_inspect_command().into(),
        )
    }

    #[must_use]
    pub fn database_tool_probe_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
        known_hosts: &Path,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity,
            known_hosts,
            database_tool_probe_command().into(),
        )
    }

    pub fn database_server_probe_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
        known_hosts: &Path,
        connection: &DatabaseConnection,
    ) -> Result<Vec<OsString>, SshError> {
        let remote_command = database_server_probe_command(connection)?;
        Ok(self.arguments_for_command(host, user, identity, known_hosts, remote_command.into()))
    }

    #[must_use]
    pub fn snapshot_sqlite_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
        known_hosts: &Path,
        database_path: &str,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity,
            known_hosts,
            sqlite_snapshot_command(database_path).into(),
        )
    }

    #[must_use]
    pub fn database_disk_budget_probe_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
        known_hosts: &Path,
        database_path: &str,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity,
            known_hosts,
            database_disk_budget_probe_command(database_path).into(),
        )
    }

    pub fn sqlite3_probe_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
        known_hosts: &Path,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity,
            known_hosts,
            sqlite3_probe_command().into(),
        )
    }
}
