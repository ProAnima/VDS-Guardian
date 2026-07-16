//! Narrow system-OpenSSH adapter for pinned, read-only archive capture.

mod process;
mod push;
mod secret_identity;
mod stream;

use guardian_core::{
    DatabaseAuthentication, DatabaseConnection, DatabaseEngine, EmbeddedDatabaseCapturePort,
    EmbeddedDatabaseCaptureRequest, FilesystemCapturePort, FilesystemCaptureRequest, HostPin,
    SecretStore, SshCapabilityProbeError, SshCapabilityProbePort, SshCaptureCapabilities,
    VdsProfile,
};
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
use tempfile::NamedTempFile;
use thiserror::Error;

pub use guardian_core::CancellationHandle;
pub use push::PushResult;
pub use secret_identity::SshIdentity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedHost {
    host: String,
    port: u16,
    algorithm: String,
    public_key: String,
}

impl PinnedHost {
    pub fn parse(
        host: impl Into<String>,
        port: u16,
        algorithm: impl Into<String>,
        public_key: impl Into<String>,
    ) -> Result<Self, SshError> {
        let host = host.into();
        let algorithm = algorithm.into();
        let public_key = public_key.into();
        if !valid_host(&host) || port == 0 {
            return Err(SshError::InvalidHostPin);
        }
        HostPin::parse(&algorithm, &public_key).map_err(|_| SshError::InvalidHostPin)?;
        Ok(Self {
            host,
            port,
            algorithm,
            public_key,
        })
    }

    #[must_use]
    pub fn known_hosts_line(&self) -> String {
        format!(
            "{} {} {}\n",
            self.known_host_name(),
            self.algorithm,
            self.public_key
        )
    }

    fn target(&self, user: &SshUser) -> String {
        format!("{}@{}", user.0, self.host)
    }

    fn known_host_name(&self) -> String {
        if self.port == 22 {
            self.host.clone()
        } else {
            format!("[{}]:{}", self.host, self.port)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshUser(String);

impl SshUser {
    pub fn parse(value: impl Into<String>) -> Result<Self, SshError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
        valid.then_some(Self(value)).ok_or(SshError::InvalidUser)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCapturePlan {
    roots: Vec<String>,
}

impl RemoteCapturePlan {
    pub fn from_roots(roots: impl IntoIterator<Item = String>) -> Result<Self, SshError> {
        let roots: Vec<String> = roots.into_iter().collect();
        let valid = !roots.is_empty()
            && roots.len() <= 32
            && roots.iter().all(|root| valid_remote_root(root));
        valid
            .then_some(Self { roots })
            .ok_or(SshError::InvalidCapturePlan)
    }

    #[must_use]
    pub fn remote_command(&self) -> String {
        let roots = self
            .roots
            .iter()
            .map(|root| shell_quote(root))
            .collect::<Vec<_>>()
            .join(" ");
        format!("tar --create --file=- --zstd --numeric-owner --one-file-system -- {roots}")
    }
}

#[derive(Debug, Clone)]
pub struct SystemOpenSsh {
    binary: PathBuf,
    connect_timeout: Duration,
    idle_timeout: Duration,
    total_timeout: Duration,
    cancellation: CancellationHandle,
}

pub struct PinnedSshCaptureAdapter<'a> {
    pub ssh: &'a SystemOpenSsh,
    pub host: &'a PinnedHost,
    pub user: &'a SshUser,
    pub identity_file: &'a Path,
    pub maximum_output_bytes: u64,
}

pub struct PinnedSshCapabilityProbe<'a> {
    pub ssh: &'a SystemOpenSsh,
    pub credentials: &'a dyn SecretStore,
}

impl SshCapabilityProbePort for PinnedSshCapabilityProbe<'_> {
    fn probe(
        &self,
        profile: &VdsProfile,
    ) -> Result<SshCaptureCapabilities, SshCapabilityProbeError> {
        profile
            .validate()
            .map_err(|_| SshCapabilityProbeError::Rejected)?;
        let host = PinnedHost::parse(
            &profile.endpoint.host,
            profile.endpoint.port,
            &profile.endpoint.host_pin.algorithm,
            &profile.endpoint.host_pin.public_key_base64,
        )
        .map_err(|_| SshCapabilityProbeError::Rejected)?;
        let user = SshUser::parse(&profile.endpoint.user)
            .map_err(|_| SshCapabilityProbeError::Rejected)?;
        let identity = SshIdentity::from_store(self.credentials, &profile.credential_id)
            .map_err(|_| SshCapabilityProbeError::Unavailable)?;
        let capabilities = self
            .ssh
            .probe_tar_zstd(&host, &user, identity.path())
            .map_err(|_| SshCapabilityProbeError::Unavailable)?;
        Ok(SshCaptureCapabilities {
            tar_zstd: capabilities.tar_zstd,
        })
    }
}

impl FilesystemCapturePort for PinnedSshCaptureAdapter<'_> {
    fn capture_to(
        &self,
        request: &FilesystemCaptureRequest,
        destination: &Path,
    ) -> Result<(), guardian_core::CapturePortError> {
        let plan = RemoteCapturePlan::from_roots(request.roots.clone())
            .map_err(|_| guardian_core::CapturePortError::Transport)?;
        self.ssh
            .capture_to(
                self.host,
                self.user,
                self.identity_file,
                &plan,
                destination,
                self.maximum_output_bytes,
            )
            .map(|_| ())
            .map_err(|_| guardian_core::CapturePortError::Transport)
    }
}

pub struct PinnedEmbeddedDatabaseCaptureAdapter<'a> {
    pub ssh: &'a SystemOpenSsh,
    pub host: &'a PinnedHost,
    pub user: &'a SshUser,
    pub identity_file: &'a Path,
    pub maximum_output_bytes: u64,
}

impl EmbeddedDatabaseCapturePort for PinnedEmbeddedDatabaseCaptureAdapter<'_> {
    fn capture_to(
        &self,
        request: &EmbeddedDatabaseCaptureRequest,
        destination: &Path,
    ) -> Result<(), guardian_core::CapturePortError> {
        self.ssh
            .snapshot_sqlite_to(
                self.host,
                self.user,
                self.identity_file,
                &request.database_path,
                destination,
                self.maximum_output_bytes,
            )
            .map(|_| ())
            .map_err(|_| guardian_core::CapturePortError::Transport)
    }
}

impl Default for SystemOpenSsh {
    fn default() -> Self {
        Self {
            binary: PathBuf::from("ssh"),
            connect_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(5 * 60),
            total_timeout: Duration::from_secs(15 * 60),
            cancellation: CancellationHandle::default(),
        }
    }
}

impl SystemOpenSsh {
    #[must_use]
    pub fn with_binary(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            connect_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(5 * 60),
            total_timeout: Duration::from_secs(15 * 60),
            cancellation: CancellationHandle::default(),
        }
    }

    #[must_use]
    pub fn with_connect_timeout(mut self, connect_timeout: Duration) -> Self {
        self.connect_timeout = connect_timeout;
        self
    }

    #[must_use]
    pub fn with_total_timeout(mut self, total_timeout: Duration) -> Self {
        self.total_timeout = total_timeout;
        self
    }

    #[must_use]
    pub fn with_idle_timeout(mut self, idle_timeout: Duration) -> Self {
        self.idle_timeout = idle_timeout;
        self
    }

    /// Ties this adapter's remote operations to an operator-triggered
    /// cancellation signal. Checked on every poll tick in the same wait
    /// loops that already enforce connect/idle/total timeouts — see
    /// `docs/adr/0010-operator-triggered-cancellation.md`.
    #[must_use]
    pub fn with_cancellation(mut self, cancellation: CancellationHandle) -> Self {
        self.cancellation = cancellation;
        self
    }

    /// Lets a composition that already holds this adapter decide its own
    /// terminal audit state (cancelled vs. failed) without needing a
    /// separate copy of the same `CancellationHandle` threaded through
    /// alongside it.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    pub fn capture_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        plan: &RemoteCapturePlan,
        destination: &Path,
        maximum_output_bytes: u64,
    ) -> Result<CaptureResult, SshError> {
        self.run_to(
            host,
            user,
            identity_file,
            plan.remote_command().into(),
            destination,
            Some(maximum_output_bytes),
        )
    }

    pub fn inspect_docker_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        destination: &Path,
        maximum_output_bytes: u64,
    ) -> Result<CaptureResult, SshError> {
        self.run_to(
            host,
            user,
            identity_file,
            docker_inspect_command().into(),
            destination,
            Some(maximum_output_bytes),
        )
    }

    pub fn probe_database_tools_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        destination: &Path,
        maximum_output_bytes: u64,
    ) -> Result<CaptureResult, SshError> {
        self.run_to(
            host,
            user,
            identity_file,
            database_tool_probe_command().into(),
            destination,
            Some(maximum_output_bytes),
        )
    }

    pub fn snapshot_sqlite_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        database_path: &str,
        destination: &Path,
        maximum_output_bytes: u64,
    ) -> Result<CaptureResult, SshError> {
        self.run_to(
            host,
            user,
            identity_file,
            sqlite_snapshot_command(database_path).into(),
            destination,
            Some(maximum_output_bytes),
        )
    }

    pub fn probe_database_disk_budget_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        database_path: &str,
        destination: &Path,
        maximum_output_bytes: u64,
    ) -> Result<CaptureResult, SshError> {
        self.run_to(
            host,
            user,
            identity_file,
            database_disk_budget_probe_command(database_path).into(),
            destination,
            Some(maximum_output_bytes),
        )
    }

    pub fn probe_database_server_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        connection: &DatabaseConnection,
        destination: &Path,
        maximum_output_bytes: u64,
    ) -> Result<CaptureResult, SshError> {
        let remote_command = database_server_probe_command(connection)?;
        self.run_to(
            host,
            user,
            identity_file,
            remote_command.into(),
            destination,
            Some(maximum_output_bytes),
        )
    }

    pub fn probe_zstd(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
    ) -> Result<bool, SshError> {
        let known_hosts = self.known_hosts_file(host)?;
        let child = self
            .new_command()
            .args(self.zstd_probe_arguments(host, user, identity_file, known_hosts.path()))
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
    pub fn zstd_probe_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        known_hosts: &Path,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity_file,
            known_hosts,
            zstd_probe_command().into(),
        )
    }

    fn run_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        remote_command: OsString,
        destination: &Path,
        maximum_output_bytes: Option<u64>,
    ) -> Result<CaptureResult, SshError> {
        let known_hosts = self.known_hosts_file(host)?;
        let output = match create_hardened_destination(destination) {
            Ok(output) => output,
            Err(error) => return fail_capture(destination, error),
        };
        let mut child = match self
            .new_command()
            .args(self.arguments_for_command(
                host,
                user,
                identity_file,
                known_hosts.path(),
                remote_command,
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(_) => return fail_capture(destination, SshError::LaunchFailed),
        };
        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => return fail_capture(destination, SshError::LocalIo),
        };
        let pump = match maximum_output_bytes {
            Some(maximum) => stream::CapturePump::start_limited(stdout, output, maximum),
            None => stream::CapturePump::start(stdout, output),
        };
        let status = match stream::wait_for_stream(
            child,
            self.total_timeout,
            self.idle_timeout,
            pump.activity(),
            pump.failed(),
            &self.cancellation,
        ) {
            Ok(status) => status,
            Err(stream::StreamWaitError::TimedOut) => {
                let _ = pump.finish();
                return fail_capture(destination, SshError::TimedOut);
            }
            Err(stream::StreamWaitError::IdleTimedOut) => {
                let _ = pump.finish();
                return fail_capture(destination, SshError::IdleTimedOut);
            }
            Err(stream::StreamWaitError::Cancelled) => {
                let _ = pump.finish();
                return fail_capture(destination, SshError::Cancelled);
            }
            Err(stream::StreamWaitError::Failed) => {
                let _ = pump.finish();
                return fail_capture(destination, SshError::LocalIo);
            }
        };
        if pump.finish().is_err() {
            return fail_capture(destination, SshError::LocalIo);
        };
        if !status.success() {
            return fail_capture(destination, SshError::CaptureFailed);
        }
        let bytes_written = match fs::metadata(destination) {
            Ok(metadata) => metadata.len(),
            Err(_) => return fail_capture(destination, SshError::LocalIo),
        };
        Ok(CaptureResult { bytes_written })
    }

    #[must_use]
    pub fn arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        known_hosts: &Path,
        plan: &RemoteCapturePlan,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity_file,
            known_hosts,
            plan.remote_command().into(),
        )
    }

    pub fn probe_tar_zstd(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
    ) -> Result<RemoteCapabilities, SshError> {
        let known_hosts = self.known_hosts_file(host)?;
        let child = self
            .new_command()
            .args(self.capability_probe_arguments(host, user, identity_file, known_hosts.path()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| SshError::LaunchFailed)?;
        let status = process::wait_for_exit(child, self.total_timeout, &self.cancellation)
            .map_err(map_wait_error)?;
        Ok(RemoteCapabilities {
            tar_zstd: status.success(),
        })
    }

    pub fn probe_sqlite3(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
    ) -> Result<bool, SshError> {
        let known_hosts = self.known_hosts_file(host)?;
        let child = self
            .new_command()
            .args(self.sqlite3_probe_arguments(host, user, identity_file, known_hosts.path()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| SshError::LaunchFailed)?;
        let status = process::wait_for_exit(child, self.total_timeout, &self.cancellation)
            .map_err(map_wait_error)?;
        Ok(status.success())
    }

    pub fn probe_connection(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
    ) -> Result<(), SshError> {
        let known_hosts = self.known_hosts_file(host)?;
        let child = self
            .new_command()
            .args(self.arguments_for_command(
                host,
                user,
                identity_file,
                known_hosts.path(),
                "true".into(),
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| SshError::LaunchFailed)?;
        let status = process::wait_for_exit(child, self.total_timeout, &self.cancellation)
            .map_err(map_wait_error)?;
        status
            .success()
            .then_some(())
            .ok_or(SshError::CaptureFailed)
    }

    #[must_use]
    pub fn capability_probe_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        known_hosts: &Path,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity_file,
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
        identity_file: &Path,
        known_hosts: &Path,
    ) -> Vec<OsString> {
        self.arguments_for_command(host, user, identity_file, known_hosts, "true".into())
    }

    #[must_use]
    pub fn docker_inspect_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        known_hosts: &Path,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity_file,
            known_hosts,
            docker_inspect_command().into(),
        )
    }

    #[must_use]
    pub fn database_tool_probe_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        known_hosts: &Path,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity_file,
            known_hosts,
            database_tool_probe_command().into(),
        )
    }

    pub fn database_server_probe_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        known_hosts: &Path,
        connection: &DatabaseConnection,
    ) -> Result<Vec<OsString>, SshError> {
        let remote_command = database_server_probe_command(connection)?;
        Ok(self.arguments_for_command(
            host,
            user,
            identity_file,
            known_hosts,
            remote_command.into(),
        ))
    }

    #[must_use]
    pub fn snapshot_sqlite_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        known_hosts: &Path,
        database_path: &str,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity_file,
            known_hosts,
            sqlite_snapshot_command(database_path).into(),
        )
    }

    #[must_use]
    pub fn database_disk_budget_probe_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        known_hosts: &Path,
        database_path: &str,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity_file,
            known_hosts,
            database_disk_budget_probe_command(database_path).into(),
        )
    }

    pub fn sqlite3_probe_arguments(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        known_hosts: &Path,
    ) -> Vec<OsString> {
        self.arguments_for_command(
            host,
            user,
            identity_file,
            known_hosts,
            sqlite3_probe_command().into(),
        )
    }

    fn known_hosts_file(&self, host: &PinnedHost) -> Result<NamedTempFile, SshError> {
        let mut known_hosts = NamedTempFile::new().map_err(|_| SshError::LocalIo)?;
        known_hosts
            .write_all(host.known_hosts_line().as_bytes())
            .and_then(|_| known_hosts.flush())
            .map_err(|_| SshError::LocalIo)?;
        Ok(known_hosts)
    }

    fn arguments_for_command(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        known_hosts: &Path,
        remote_command: OsString,
    ) -> Vec<OsString> {
        vec![
            "-F".into(),
            "none".into(),
            "-o".into(),
            "BatchMode=yes".into(),
            "-o".into(),
            format!("ConnectTimeout={}", timeout_seconds(self.connect_timeout)).into(),
            "-o".into(),
            "StrictHostKeyChecking=yes".into(),
            "-o".into(),
            format!("UserKnownHostsFile={}", known_hosts.display()).into(),
            "-o".into(),
            "GlobalKnownHostsFile=none".into(),
            "-o".into(),
            "PasswordAuthentication=no".into(),
            "-o".into(),
            "KbdInteractiveAuthentication=no".into(),
            "-o".into(),
            "PreferredAuthentications=publickey".into(),
            "-o".into(),
            "IdentitiesOnly=yes".into(),
            "-i".into(),
            identity_file.as_os_str().to_owned(),
            "-p".into(),
            host.port.to_string().into(),
            host.target(user).into(),
            remote_command,
        ]
    }

    /// A child spawned via `Command` inherits the parent's console/
    /// foreground process group by default on both platforms, so an
    /// operator's raw Ctrl+C would otherwise reach this child directly,
    /// racing the cooperative `cancellation`-checked kill in
    /// `process::wait_for_exit`/`stream::wait_for_stream`. Spawning into a
    /// new process group (Windows) / new POSIX process group (Unix) makes
    /// the cooperative kill path the only thing that ends this child.
    fn new_command(&self) -> Command {
        let mut command = Command::new(&self.binary);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            command.creation_flags(CREATE_NEW_PROCESS_GROUP);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        command
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteCapabilities {
    pub tar_zstd: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureResult {
    pub bytes_written: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SshError {
    #[error("SSH host pin is invalid")]
    InvalidHostPin,
    #[error("SSH user is invalid")]
    InvalidUser,
    #[error("capture roots are invalid")]
    InvalidCapturePlan,
    #[error("capture destination is unavailable")]
    DestinationUnavailable,
    #[error("unable to prepare local SSH capture")]
    LocalIo,
    #[error("unable to start system OpenSSH")]
    LaunchFailed,
    #[error("remote capture failed")]
    CaptureFailed,
    #[error("SSH capture exceeded its total time limit")]
    TimedOut,
    #[error("SSH capture exceeded its idle stream time limit")]
    IdleTimedOut,
    #[error("SSH operation was cancelled by the operator")]
    Cancelled,
    #[error("SSH credential is unavailable")]
    CredentialUnavailable,
    #[error("SSH credential is not a supported unencrypted SSH private key")]
    InvalidCredential,
    #[error("temporary SSH identity file could not be prepared")]
    TemporaryIdentityFile,
    #[error("database connection is invalid")]
    InvalidDatabaseConnection,
    #[error("database authentication mode is not supported over SSH")]
    UnsupportedDatabaseAuthentication,
    #[error("pushed byte count did not match the payload's verified length")]
    ByteCountMismatch,
}

fn fail_capture(destination: &Path, error: SshError) -> Result<CaptureResult, SshError> {
    let _ = fs::remove_file(destination);
    Err(error)
}

/// Creates the local capture destination and narrows it to the current user
/// before any captured bytes reach it: unlike the short-lived identity file
/// `secret_identity::restrict_permissions` also hardens, a capture stream
/// can hold gigabytes of backup content open for minutes.
fn create_hardened_destination(destination: &Path) -> Result<fs::File, SshError> {
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|_| SshError::DestinationUnavailable)?;
    secret_identity::restrict_permissions(destination)
        .map_err(|_| SshError::DestinationUnavailable)?;
    Ok(output)
}

fn map_wait_error(error: process::WaitError) -> SshError {
    match error {
        process::WaitError::TimedOut => SshError::TimedOut,
        process::WaitError::Cancelled => SshError::Cancelled,
        process::WaitError::Failed => SshError::LocalIo,
    }
}

fn timeout_seconds(timeout: Duration) -> u64 {
    timeout.as_secs().max(1)
}

fn docker_inspect_command() -> &'static str {
    "ids=$(docker ps --all --quiet --no-trunc) || exit 1; [ -z \"$ids\" ] || printf '%s\\n' \"$ids\" | xargs -r docker inspect --"
}

fn sqlite_snapshot_command(database_path: &str) -> String {
    let path = shell_quote(database_path);
    format!(
        "[ -f {path} ] || exit 1; tmp=$(mktemp) || exit 1; sqlite3 {path} \".backup '$tmp'\" && zstd -q -c \"$tmp\"; status=$?; rm -f \"$tmp\"; exit $status"
    )
}

fn database_disk_budget_probe_command(database_path: &str) -> String {
    let path = shell_quote(database_path);
    format!(
        "size=$(stat -c%s {path} 2>/dev/null) && [ -n \"$size\" ] && free=$(df -Pk {path} | tail -n 1 | awk '{{print $4}}') && printf '%s %s\\n' \"$size\" \"$free\""
    )
}

fn sqlite3_probe_command() -> &'static str {
    "command -v sqlite3 >/dev/null 2>&1"
}

fn zstd_probe_command() -> &'static str {
    "command -v zstd >/dev/null 2>&1"
}

fn database_tool_probe_command() -> &'static str {
    "if command -v pg_dump >/dev/null 2>&1; then printf 'postgresql\\t'; pg_dump --version || exit 1; fi; if command -v mysqldump >/dev/null 2>&1; then printf 'mysql\\t'; mysqldump --version || exit 1; fi"
}

fn database_server_probe_command(connection: &DatabaseConnection) -> Result<String, SshError> {
    connection
        .validate()
        .map_err(|_| SshError::InvalidDatabaseConnection)?;
    if !matches!(connection.authentication, DatabaseAuthentication::SshPeer) {
        return Err(SshError::UnsupportedDatabaseAuthentication);
    }
    let host = shell_quote(&connection.host);
    let port = shell_quote(&connection.port.to_string());
    let database = shell_quote(&connection.database_name);
    Ok(match connection.engine {
        DatabaseEngine::PostgreSql => format!(
            "psql --no-password --tuples-only --no-align --host {host} --port {port} --dbname {database} --command 'SHOW server_version'"
        ),
        DatabaseEngine::MySql => format!(
            "mysql --protocol=TCP --skip-password --batch --skip-column-names --host {host} --port {port} --database {database} --execute 'SELECT VERSION()'"
        ),
    })
}

fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && !host.starts_with(['-', '.'])
        && !host.ends_with(['-', '.'])
        && !host.contains("..")
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
}

fn valid_remote_root(root: &str) -> bool {
    root == "/"
        || (root.starts_with('/')
            && root.len() <= 1_024
            && !root.contains(['\0', '\n', '\r', '\\'])
            && root
                .split('/')
                .skip(1)
                .all(|segment| !matches!(segment, "" | "." | "..")))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use super::{SystemOpenSsh, create_hardened_destination};

    /// Not a check of the OS-level process-group semantics themselves (that
    /// would need platform-specific introspection this codebase has no
    /// dependency for) -- just a regression guard that the isolation flags
    /// `new_command` applies don't break ordinary spawning, since an invalid
    /// flag value would silently fail every SSH operation this crate makes.
    #[test]
    fn new_command_applies_process_group_isolation_and_still_spawns()
    -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(windows)]
        let ssh = SystemOpenSsh::with_binary("cmd.exe");
        #[cfg(not(windows))]
        let ssh = SystemOpenSsh::with_binary("sh");
        let mut command = ssh.new_command();
        #[cfg(windows)]
        command.args(["/C", "exit 0"]);
        #[cfg(not(windows))]
        command.args(["-c", "exit 0"]);
        let status = command.status()?;
        assert!(status.success());
        Ok(())
    }

    #[test]
    fn create_hardened_destination_narrows_the_captured_files_permissions()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let destination = directory.path().join("captured.tar.zst.enc");
        create_hardened_destination(&destination)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&destination)?.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        #[cfg(windows)]
        {
            let system_root = std::env::var_os("SystemRoot")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"));
            let output = std::process::Command::new(system_root.join(r"System32\icacls.exe"))
                .arg(&destination)
                .output()?;
            let rendered = String::from_utf8_lossy(&output.stdout);
            assert!(output.status.success());
            assert!(!rendered.contains("(I)"));
        }
        Ok(())
    }

    #[test]
    fn create_hardened_destination_fails_closed_when_the_path_already_exists()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let destination = directory.path().join("captured.tar.zst.enc");
        std::fs::write(&destination, b"existing")?;
        assert!(create_hardened_destination(&destination).is_err());
        Ok(())
    }
}
