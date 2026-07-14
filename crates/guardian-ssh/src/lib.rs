//! Narrow system-OpenSSH adapter for pinned, read-only archive capture.

use guardian_core::{FilesystemCapturePort, FilesystemCaptureRequest, HostPin};
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use tempfile::NamedTempFile;
use thiserror::Error;

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
}

pub struct PinnedSshCaptureAdapter<'a> {
    pub ssh: &'a SystemOpenSsh,
    pub host: &'a PinnedHost,
    pub user: &'a SshUser,
    pub identity_file: &'a Path,
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
            .capture_to(self.host, self.user, self.identity_file, &plan, destination)
            .map(|_| ())
            .map_err(|_| guardian_core::CapturePortError::Transport)
    }
}

impl Default for SystemOpenSsh {
    fn default() -> Self {
        Self {
            binary: PathBuf::from("ssh"),
        }
    }
}

impl SystemOpenSsh {
    #[must_use]
    pub fn with_binary(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
        }
    }

    pub fn capture_to(
        &self,
        host: &PinnedHost,
        user: &SshUser,
        identity_file: &Path,
        plan: &RemoteCapturePlan,
        destination: &Path,
    ) -> Result<CaptureResult, SshError> {
        let mut known_hosts = NamedTempFile::new().map_err(|_| SshError::LocalIo)?;
        known_hosts
            .write_all(host.known_hosts_line().as_bytes())
            .and_then(|_| known_hosts.flush())
            .map_err(|_| SshError::LocalIo)?;
        let output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|_| SshError::DestinationUnavailable)?;
        let status = Command::new(&self.binary)
            .args(self.arguments(host, user, identity_file, known_hosts.path(), plan))
            .stdin(Stdio::null())
            .stdout(Stdio::from(output))
            .stderr(Stdio::null())
            .status();
        let status = match status {
            Ok(status) => status,
            Err(_) => {
                let _ = fs::remove_file(destination);
                return Err(SshError::LaunchFailed);
            }
        };
        if !status.success() {
            let _ = fs::remove_file(destination);
            return Err(SshError::CaptureFailed);
        }
        let bytes_written = fs::metadata(destination)
            .map_err(|_| SshError::LocalIo)?
            .len();
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
        vec![
            "-F".into(),
            "none".into(),
            "-o".into(),
            "BatchMode=yes".into(),
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
            plan.remote_command().into(),
        ]
    }
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
