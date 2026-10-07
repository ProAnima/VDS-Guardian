//! The validated inputs of every SSH operation: the pinned host, the login user and the remote
//! roots a capture may read.

use crate::SshError;
use guardian_core::HostPin;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedHost {
    host: String,
    pub(crate) port: u16,
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

    pub(crate) fn target(&self, user: &SshUser) -> String {
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

pub(crate) fn valid_host(host: &str) -> bool {
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

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
