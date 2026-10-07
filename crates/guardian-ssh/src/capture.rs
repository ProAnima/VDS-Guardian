//! Streaming a remote command's output into a hardened local file: archive capture, Docker
//! inventory, directory listings and SQLite snapshots.

use crate::remote_commands::{
    database_disk_budget_probe_command, database_server_probe_command, database_tool_probe_command,
    docker_inspect_command, sqlite_snapshot_command,
};
use crate::{
    PinnedHost, RemoteCapturePlan, SshError, SshIdentity, SshUser, SystemOpenSsh, remote_browser,
    secret_identity, stream,
};
use guardian_core::DatabaseConnection;
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    path::Path,
    process::Stdio,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureResult {
    pub bytes_written: u64,
}

impl SystemOpenSsh {
    pub fn capture_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
        plan: &RemoteCapturePlan,
        destination: &Path,
        maximum_output_bytes: u64,
    ) -> Result<CaptureResult, SshError> {
        self.run_to(
            host,
            user,
            identity,
            plan.remote_command().into(),
            destination,
            Some(maximum_output_bytes),
        )
    }

    pub fn inspect_docker_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
        destination: &Path,
        maximum_output_bytes: u64,
    ) -> Result<CaptureResult, SshError> {
        self.run_to(
            host,
            user,
            identity,
            docker_inspect_command().into(),
            destination,
            Some(maximum_output_bytes),
        )
    }

    pub fn browse_directory_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
        directory: &guardian_core::RemotePath,
        destination: &Path,
        maximum_output_bytes: u64,
    ) -> Result<CaptureResult, SshError> {
        self.run_to(
            host,
            user,
            identity,
            remote_browser::browse_command(directory).into(),
            destination,
            Some(maximum_output_bytes),
        )
    }

    pub fn probe_database_tools_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
        destination: &Path,
        maximum_output_bytes: u64,
    ) -> Result<CaptureResult, SshError> {
        self.run_to(
            host,
            user,
            identity,
            database_tool_probe_command().into(),
            destination,
            Some(maximum_output_bytes),
        )
    }

    pub fn snapshot_sqlite_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
        database_path: &str,
        destination: &Path,
        maximum_output_bytes: u64,
    ) -> Result<CaptureResult, SshError> {
        self.run_to(
            host,
            user,
            identity,
            sqlite_snapshot_command(database_path).into(),
            destination,
            Some(maximum_output_bytes),
        )
    }

    pub fn probe_database_disk_budget_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
        database_path: &str,
        destination: &Path,
        maximum_output_bytes: u64,
    ) -> Result<CaptureResult, SshError> {
        self.run_to(
            host,
            user,
            identity,
            database_disk_budget_probe_command(database_path).into(),
            destination,
            Some(maximum_output_bytes),
        )
    }

    pub fn probe_database_server_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
        connection: &DatabaseConnection,
        destination: &Path,
        maximum_output_bytes: u64,
    ) -> Result<CaptureResult, SshError> {
        let remote_command = database_server_probe_command(connection)?;
        self.run_to(
            host,
            user,
            identity,
            remote_command.into(),
            destination,
            Some(maximum_output_bytes),
        )
    }

    /// Runs `remote_command` and streams its stdout into a new, owner-only `destination`. On any
    /// failure after the pin is written the partial file is removed.
    fn run_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
        remote_command: OsString,
        destination: &Path,
        maximum_output_bytes: Option<u64>,
    ) -> Result<CaptureResult, SshError> {
        let known_hosts = self.known_hosts_file(host)?;
        let arguments =
            self.arguments_for_command(host, user, identity, known_hosts.as_ref(), remote_command);
        if let Err(error) = self.stream_into(identity, arguments, destination, maximum_output_bytes)
        {
            return fail_capture(destination, error);
        }
        match fs::metadata(destination) {
            Ok(metadata) => Ok(CaptureResult {
                bytes_written: metadata.len(),
            }),
            Err(_) => fail_capture(destination, SshError::LocalIo),
        }
    }

    fn stream_into(
        &self,
        identity: &SshIdentity,
        arguments: Vec<OsString>,
        destination: &Path,
        maximum_output_bytes: Option<u64>,
    ) -> Result<(), SshError> {
        let output = create_hardened_destination(destination)?;
        // Holds the password broker (if any) until the child has finished.
        let mut ssh_command = self.new_command(identity)?;
        let mut child = ssh_command
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| SshError::LaunchFailed)?;
        let stdout = child.stdout.take().ok_or(SshError::LocalIo)?;
        let pump = match maximum_output_bytes {
            Some(maximum) => stream::CapturePump::start_limited(stdout, output, maximum),
            None => stream::CapturePump::start(stdout, output),
        };
        let waited = stream::wait_for_stream(
            child,
            self.total_timeout,
            self.idle_timeout,
            pump.activity(),
            pump.failed(),
            &self.cancellation,
        );
        let finished = pump.finish();
        let status = waited.map_err(stream_wait_error)?;
        finished.map_err(|_| SshError::LocalIo)?;
        status
            .success()
            .then_some(())
            .ok_or(SshError::CaptureFailed)
    }

    #[must_use]
    pub fn arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
        known_hosts: &Path,
        plan: &RemoteCapturePlan,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity,
            known_hosts,
            plan.remote_command().into(),
        )
    }
}

fn stream_wait_error(error: stream::StreamWaitError) -> SshError {
    match error {
        stream::StreamWaitError::TimedOut => SshError::TimedOut,
        stream::StreamWaitError::IdleTimedOut => SshError::IdleTimedOut,
        stream::StreamWaitError::Cancelled => SshError::Cancelled,
        stream::StreamWaitError::Failed => SshError::LocalIo,
    }
}

fn fail_capture(destination: &Path, error: SshError) -> Result<CaptureResult, SshError> {
    let _ = fs::remove_file(destination);
    Err(error)
}

/// Creates the local capture destination and narrows it to the current user
/// before any captured bytes reach it: unlike the short-lived identity file
/// `secret_identity::restrict_permissions` also hardens, a capture stream
/// can hold gigabytes of backup content open for minutes.
pub(crate) fn create_hardened_destination(destination: &Path) -> Result<fs::File, SshError> {
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|_| SshError::DestinationUnavailable)?;
    secret_identity::restrict_permissions(destination)
        .map_err(|_| SshError::DestinationUnavailable)?;
    Ok(output)
}
