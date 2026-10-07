//! Launching the system `ssh` binary: timeouts, the throwaway `known_hosts` pin, the
//! authentication options for each login kind and, for a password login, the askpass broker.

use crate::{CancellationHandle, PinnedHost, SshError, SshIdentity, SshUser};
use std::{
    ffi::OsString,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use tempfile::{NamedTempFile, TempPath};

#[derive(Debug, Clone)]
pub struct SystemOpenSsh {
    binary: PathBuf,
    pub(crate) connect_timeout: Duration,
    pub(crate) idle_timeout: Duration,
    pub(crate) total_timeout: Duration,
    pub(crate) cancellation: CancellationHandle,
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
}

impl SystemOpenSsh {
    pub(crate) fn known_hosts_file(&self, host: &PinnedHost) -> Result<TempPath, SshError> {
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

    pub(crate) fn arguments_for_command(
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
    pub(crate) fn new_command(&self, identity: &SshIdentity) -> Result<SshCommand, SshError> {
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

pub(crate) fn timeout_seconds(timeout: Duration) -> u64 {
    timeout.as_secs().max(1)
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
