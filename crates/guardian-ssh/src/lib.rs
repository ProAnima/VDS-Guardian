//! Narrow system-OpenSSH adapter for pinned, read-only archive capture.

mod host_key_scan;
mod process;
mod push;
mod remote_browser;
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
use tempfile::{NamedTempFile, TempPath};
use thiserror::Error;

pub use guardian_core::CancellationHandle;
pub use host_key_scan::{ScannedHostKey, openssh_fingerprint, scan_host_key};
pub use push::{PushResult, ReplacementTarget, StagingTarget};
pub use remote_browser::SshRemoteBrowserAdapter;
pub use secret_identity::{
    SshIdentity, init_password_helper, password_logins_available,
    register_current_executable_as_password_helper, register_password_helper,
};

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
    pub identity: &'a SshIdentity,
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
            .probe_tar_zstd(&host, &user, &identity)
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
                self.identity,
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
    pub identity: &'a SshIdentity,
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
                self.identity,
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

    pub fn probe_zstd(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
    ) -> Result<bool, SshError> {
        let known_hosts = self.known_hosts_file(host)?;
        let mut ssh_command = self.new_command(identity)?;
        let child = ssh_command
            .args(self.zstd_probe_arguments(host, user, identity, known_hosts.as_ref()))
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
        let output = match create_hardened_destination(destination) {
            Ok(output) => output,
            Err(error) => return fail_capture(destination, error),
        };
        let mut ssh_command = match self.new_command(identity) {
            Ok(command) => command,
            Err(error) => return fail_capture(destination, error),
        };
        let mut child = match ssh_command
            .args(self.arguments_for_command(
                host,
                user,
                identity,
                known_hosts.as_ref(),
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

    pub fn probe_tar_zstd(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
    ) -> Result<RemoteCapabilities, SshError> {
        let known_hosts = self.known_hosts_file(host)?;
        let mut ssh_command = self.new_command(identity)?;
        let child = ssh_command
            .args(self.capability_probe_arguments(host, user, identity, known_hosts.as_ref()))
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
        identity: &SshIdentity,
    ) -> Result<bool, SshError> {
        let known_hosts = self.known_hosts_file(host)?;
        let mut ssh_command = self.new_command(identity)?;
        let child = ssh_command
            .args(self.sqlite3_probe_arguments(host, user, identity, known_hosts.as_ref()))
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
        identity: &SshIdentity,
    ) -> Result<(), SshError> {
        let known_hosts = self.known_hosts_file(host)?;
        let mut ssh_command = self.new_command(identity)?;
        let child = ssh_command
            .args(self.arguments_for_command(
                host,
                user,
                identity,
                known_hosts.as_ref(),
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

    fn known_hosts_file(&self, host: &PinnedHost) -> Result<TempPath, SshError> {
        let mut known_hosts = NamedTempFile::new().map_err(|_| SshError::LocalIo)?;
        if !usable_known_hosts_path(known_hosts.path()) {
            return Err(SshError::LocalIo);
        }
        known_hosts
            .write_all(host.known_hosts_line().as_bytes())
            .and_then(|_| known_hosts.flush())
            .and_then(|_| known_hosts.as_file().sync_all())
            .map_err(|_| SshError::LocalIo)?;
        Ok(known_hosts.into_temp_path())
    }

    fn arguments_for_command(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity: &SshIdentity,
        known_hosts: &Path,
        remote_command: OsString,
    ) -> Vec<OsString> {
        let mut arguments: Vec<OsString> = vec![
            "-F".into(),
            "none".into(),
            "-o".into(),
            format!("ConnectTimeout={}", timeout_seconds(self.connect_timeout)).into(),
            "-o".into(),
            "StrictHostKeyChecking=yes".into(),
            "-o".into(),
            known_hosts_option(known_hosts).into(),
            "-o".into(),
            "GlobalKnownHostsFile=none".into(),
        ];
        arguments.extend(authentication_arguments(identity));
        arguments.extend([
            "-p".into(),
            host.port.to_string().into(),
            host.target(user).into(),
            remote_command,
        ]);
        arguments
    }

    /// A child spawned via `Command` inherits the parent's console/
    /// foreground process group by default on both platforms, so an
    /// operator's raw Ctrl+C would otherwise reach this child directly,
    /// racing the cooperative `cancellation`-checked kill in
    /// `process::wait_for_exit`/`stream::wait_for_stream`. Spawning into a
    /// new process group (Windows) / new POSIX process group (Unix) makes
    /// the cooperative kill path the only thing that ends this child.
    fn new_command(&self, identity: &SshIdentity) -> Result<SshCommand, SshError> {
        let mut command = Command::new(&self.binary);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let broker = self.attach_password_broker(&mut command, identity)?;
        Ok(SshCommand {
            command,
            _broker: broker,
        })
    }

    /// For a password login only: start the one-shot broker and point OpenSSH at the helper.
    /// The environment carries the helper path, a loopback port and a one-time token; the
    /// password itself never leaves the broker except over that single authenticated connection.
    fn attach_password_broker(
        &self,
        command: &mut Command,
        identity: &SshIdentity,
    ) -> Result<Option<guardian_askpass::Broker>, SshError> {
        let SshIdentity::Password(login) = identity else {
            return Ok(None);
        };
        let lifetime = self.connect_timeout + PASSWORD_BROKER_GRACE;
        let broker = guardian_askpass::Broker::start(login.password(), lifetime)
            .map_err(|_| SshError::LaunchFailed)?;
        command
            .env("SSH_ASKPASS", login.askpass_program())
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env(guardian_askpass::PORT_VARIABLE, broker.port().to_string())
            .env(guardian_askpass::TOKEN_VARIABLE, broker.token());
        Ok(Some(broker))
    }
}

/// A prepared `ssh` command. For a password login it owns the broker, so it must stay alive
/// until the child has finished authenticating; dropping it closes the broker.
pub(crate) struct SshCommand {
    command: Command,
    _broker: Option<guardian_askpass::Broker>,
}

impl std::ops::Deref for SshCommand {
    type Target = Command;
    fn deref(&self) -> &Command {
        &self.command
    }
}

impl std::ops::DerefMut for SshCommand {
    fn deref_mut(&mut self) -> &mut Command {
        &mut self.command
    }
}

const PASSWORD_BROKER_GRACE: Duration = Duration::from_secs(30);

/// Key and agent logins stay non-interactive (`BatchMode`) and key-only; a password login is the
/// one mode that enables password authentication, with exactly one attempt so a wrong password
/// can never trigger retries or lock the account.
fn authentication_arguments(identity: &SshIdentity) -> Vec<OsString> {
    let options: &[&str] = if identity.is_password() {
        &[
            "BatchMode=no",
            "PasswordAuthentication=yes",
            "KbdInteractiveAuthentication=no",
            "PubkeyAuthentication=no",
            "PreferredAuthentications=password",
            "NumberOfPasswordPrompts=1",
        ]
    } else {
        &[
            "BatchMode=yes",
            "PasswordAuthentication=no",
            "KbdInteractiveAuthentication=no",
            "PreferredAuthentications=publickey",
            "IdentitiesOnly=yes",
        ]
    };
    let mut arguments: Vec<OsString> = options
        .iter()
        .flat_map(|option| ["-o".into(), (*option).into()])
        .collect();
    if let Some(path) = identity.key_path() {
        arguments.extend(["-i".into(), path.as_os_str().to_owned()]);
    }
    arguments
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
    #[error("the server did not present a usable public host key")]
    HostKeyUnavailable,
    #[error("SSH credential is unavailable")]
    CredentialUnavailable,
    #[error("SSH credential is not a supported SSH key, agent key or login password")]
    InvalidCredential,
    #[error("temporary SSH identity file could not be prepared")]
    TemporaryIdentityFile,
    #[error("the password helper is missing, so a password login cannot be used safely")]
    AskpassUnavailable,
    #[error("database connection is invalid")]
    InvalidDatabaseConnection,
    #[error("database authentication mode is not supported over SSH")]
    UnsupportedDatabaseAuthentication,
    #[error("pushed byte count did not match the payload's verified length")]
    ByteCountMismatch,
    #[error("replacement failed and the original data was restored")]
    ReplacementRolledBack,
    #[error("replacement rollback could not be completed safely")]
    ReplacementRollbackFailed,
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

/// `UserKnownHostsFile` is split on whitespace and `%`-expanded by OpenSSH, so the path is quoted
/// and any path that quoting cannot carry safely is refused when the file is created.
pub(crate) fn known_hosts_option(path: &Path) -> String {
    format!("UserKnownHostsFile=\"{}\"", path.display())
}

pub(crate) fn usable_known_hosts_path(path: &Path) -> bool {
    path.to_str()
        .is_some_and(|text| !text.contains(['"', '%', '\n', '\r']))
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
    use super::{SshIdentity, SystemOpenSsh, create_hardened_destination};
    use std::{path::PathBuf, process::Command};

    /// Not a check of the OS-level process-group semantics themselves (that
    /// would need platform-specific introspection this codebase has no
    /// dependency for) -- just a regression guard that the isolation flags
    /// `new_command` applies don't break ordinary spawning, since an invalid
    /// flag value would silently fail every SSH operation this crate makes.
    #[test]
    fn known_hosts_paths_are_quoted_and_unquotable_paths_are_refused() {
        use std::path::Path;
        assert_eq!(
            super::known_hosts_option(Path::new("C:/Users/Jane Doe/known hosts")),
            "UserKnownHostsFile=\"C:/Users/Jane Doe/known hosts\""
        );
        assert!(super::usable_known_hosts_path(Path::new(
            "C:/Users/Jane Doe/AppData/kh"
        )));
        for bad in ["C:/a\"b", "C:/100%/kh", "/tmp/a\nb"] {
            assert!(!super::usable_known_hosts_path(Path::new(bad)), "{bad:?}");
        }
    }

    fn environment(command: &Command) -> Vec<(String, String)> {
        command
            .get_envs()
            .filter_map(|(key, value)| {
                Some((
                    key.to_string_lossy().into_owned(),
                    value?.to_string_lossy().into_owned(),
                ))
            })
            .collect()
    }

    #[test]
    fn a_password_command_carries_only_the_helper_port_and_token_in_its_environment()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity =
            SshIdentity::password_with_helper(b"S3cret-Pass!", PathBuf::from("the-helper"))?;
        let command = SystemOpenSsh::default().new_command(&identity)?;
        let variables = environment(&command);
        let value = |name: &str| {
            variables
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        };
        assert_eq!(value("SSH_ASKPASS").as_deref(), Some("the-helper"));
        assert_eq!(value("SSH_ASKPASS_REQUIRE").as_deref(), Some("force"));
        assert!(
            value(guardian_askpass::PORT_VARIABLE)
                .is_some_and(|port| port.parse::<u16>().is_ok_and(|port| port > 0))
        );
        assert!(
            value(guardian_askpass::TOKEN_VARIABLE)
                .is_some_and(|token| guardian_askpass::is_valid_token(&token))
        );
        assert!(
            !variables
                .iter()
                .any(|(key, value)| key.contains("S3cret") || value.contains("S3cret")),
            "the password must not be in the environment"
        );
        assert_eq!(
            command.get_args().count(),
            0,
            "arguments are added by the caller, never the password"
        );
        Ok(())
    }

    #[test]
    fn every_password_command_gets_its_own_one_time_token() -> Result<(), Box<dyn std::error::Error>>
    {
        let identity =
            SshIdentity::password_with_helper(b"S3cret-Pass!", PathBuf::from("the-helper"))?;
        let ssh = SystemOpenSsh::default();
        let (first, second) = (ssh.new_command(&identity)?, ssh.new_command(&identity)?);
        let token = |command: &Command| {
            environment(command)
                .into_iter()
                .find(|(key, _)| key == guardian_askpass::TOKEN_VARIABLE)
        };
        assert_ne!(token(&first), token(&second));
        Ok(())
    }

    #[test]
    fn dropping_a_password_command_closes_its_broker() -> Result<(), Box<dyn std::error::Error>> {
        let identity =
            SshIdentity::password_with_helper(b"S3cret-Pass!", PathBuf::from("the-helper"))?;
        let command = SystemOpenSsh::default().new_command(&identity)?;
        let variables = environment(&command);
        let find = |name: &str| {
            variables
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        };
        let (port, token) = (
            find(guardian_askpass::PORT_VARIABLE).unwrap_or_default(),
            find(guardian_askpass::TOKEN_VARIABLE).unwrap_or_default(),
        );
        drop(command);
        assert!(guardian_askpass::request_password("Password:", &port, &token).is_err());
        Ok(())
    }

    #[test]
    fn a_key_command_never_touches_the_askpass_environment()
    -> Result<(), Box<dyn std::error::Error>> {
        let marker = SshIdentity::encode_agent_identity("ssh-ed25519", &agent_blob())?;
        let identity = SshIdentity::from_store(
            &FixedStore(marker),
            &guardian_core::CredentialId::parse("credential-agent")?,
        )?;
        let command = SystemOpenSsh::default().new_command(&identity)?;
        assert!(environment(&command).is_empty());
        Ok(())
    }

    fn agent_blob() -> String {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let mut blob = Vec::new();
        blob.extend_from_slice(&11_u32.to_be_bytes());
        blob.extend_from_slice(b"ssh-ed25519");
        blob.push(1);
        STANDARD.encode(blob)
    }

    struct FixedStore(Vec<u8>);

    impl guardian_core::SecretStore for FixedStore {
        fn load(
            &self,
            _: &guardian_core::CredentialId,
        ) -> Result<Option<guardian_core::SecretValue>, guardian_core::SecretStoreError> {
            Ok(Some(guardian_core::SecretValue::new(self.0.clone())))
        }
        fn store(
            &self,
            _: &guardian_core::CredentialId,
            _: &guardian_core::SecretValue,
        ) -> Result<(), guardian_core::SecretStoreError> {
            Ok(())
        }
        fn delete(
            &self,
            _: &guardian_core::CredentialId,
        ) -> Result<(), guardian_core::SecretStoreError> {
            Ok(())
        }
    }

    #[test]
    fn new_command_applies_process_group_isolation_and_still_spawns()
    -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(windows)]
        let ssh = SystemOpenSsh::with_binary("cmd.exe");
        #[cfg(not(windows))]
        let ssh = SystemOpenSsh::with_binary("sh");
        // A password identity exercises the broker path as well; the helper is never run.
        let identity = crate::SshIdentity::password_with_helper(
            b"unused",
            std::path::PathBuf::from("unused-helper"),
        )?;
        let mut command = ssh.new_command(&identity)?;
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
