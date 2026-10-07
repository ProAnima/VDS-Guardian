//! Narrow system-OpenSSH adapter for pinned, read-only archive capture.

mod adapters;
mod capture;
mod error;
mod host_key_scan;
mod openssh;
mod probes;
mod process;
mod push;
mod remote_browser;
mod remote_commands;
mod secret_identity;
mod stream;
mod target;

pub use adapters::{
    PinnedEmbeddedDatabaseCaptureAdapter, PinnedSshCapabilityProbe, PinnedSshCaptureAdapter,
};
pub use capture::CaptureResult;
pub use error::SshError;
pub use guardian_core::CancellationHandle;
pub use host_key_scan::{ScannedHostKey, openssh_fingerprint, scan_host_key};
pub use openssh::SystemOpenSsh;
pub use probes::RemoteCapabilities;
pub use push::{PushResult, ReplacementTarget, StagingTarget};
pub use remote_browser::SshRemoteBrowserAdapter;
pub use secret_identity::{
    SshIdentity, init_password_helper, password_logins_available,
    register_current_executable_as_password_helper, register_password_helper,
};
pub use target::{PinnedHost, RemoteCapturePlan, SshUser};

pub(crate) use error::map_wait_error;
pub(crate) use openssh::{known_hosts_option, usable_known_hosts_path};
pub(crate) use target::{shell_quote, valid_host};

#[cfg(test)]
mod tests;
