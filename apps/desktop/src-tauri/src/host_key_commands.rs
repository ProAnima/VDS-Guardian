//! "Fetch host key" for the Servers form. It only retrieves what the server presents so the
//! operator can compare its fingerprint with a trusted source; nothing is pinned or trusted
//! here, and the command is deliberately not part of the MCP surface.

use guardian_ssh::{SshError, scan_host_key};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const SCAN_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanHostKeyRequest {
    host: String,
    port: u16,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeySummary {
    algorithm: String,
    public_key: String,
    fingerprint: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyFailure {
    pub code: &'static str,
    pub message: &'static str,
    pub remediation: &'static str,
}

pub async fn scan(request: ScanHostKeyRequest) -> Result<HostKeySummary, HostKeyFailure> {
    tauri::async_runtime::spawn_blocking(move || {
        scan_host_key(&request.host, request.port, SCAN_TIMEOUT)
            .map(|key| HostKeySummary {
                algorithm: key.algorithm,
                public_key: key.public_key_base64,
                fingerprint: key.fingerprint,
            })
            .map_err(map_error)
    })
    .await
    .map_err(|_| HostKeyFailure::internal())?
}

fn map_error(error: SshError) -> HostKeyFailure {
    match error {
        SshError::InvalidHostPin => HostKeyFailure::invalid_address(),
        SshError::LaunchFailed => HostKeyFailure::client_missing(),
        _ => HostKeyFailure::unreachable(),
    }
}

impl HostKeyFailure {
    fn invalid_address() -> Self {
        Self {
            code: "invalid_server_address",
            message: "The server address or port is not valid.",
            remediation: "Use a host name or IP address and a port from 1 to 65535.",
        }
    }
    fn client_missing() -> Self {
        Self {
            code: "ssh_client_missing",
            message: "The system OpenSSH client could not be started.",
            remediation: "Install the OpenSSH client, or paste the server's host key manually.",
        }
    }
    fn unreachable() -> Self {
        Self {
            code: "host_key_unavailable",
            message: "A host key could not be read from this address.",
            remediation: "Check the address and port and that the server is reachable, or paste the host key manually.",
        }
    }
    fn internal() -> Self {
        Self {
            code: "internal_error",
            message: "The desktop command did not complete.",
            remediation: "Try again and export redacted diagnostics if the problem persists.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HostKeyFailure, ScanHostKeyRequest, map_error, scan};
    use guardian_ssh::SshError;

    fn scan_code(host: &str, port: u16) -> Option<&'static str> {
        let request = ScanHostKeyRequest {
            host: host.to_owned(),
            port,
        };
        tauri::async_runtime::block_on(scan(request))
            .err()
            .map(|failure| failure.code)
    }

    /// Each of these would reach OpenSSH as an option, a second argument, a `%` token or a
    /// different destination. They are refused as an invalid address, which only the up-front
    /// validation produces; a launched `ssh` could only end in `host_key_unavailable` or
    /// `ssh_client_missing` (that no process is started is proven in guardian-ssh).
    #[test]
    fn hostile_addresses_are_refused_before_anything_is_started() {
        let overlong = format!("{}.example", "a".repeat(250));
        for hostile in [
            "-oProxyCommand=calc.exe",
            "-oProxyCommand=sh -c 'id>/tmp/pwned'",
            "vds.example -oProxyCommand=calc",
            "vds example",
            "vds.example\n-oProxyCommand=calc",
            "vds.example\r",
            "\tvds.example",
            "%h.example",
            "vds%n",
            "user@vds.example",
            "vds.example:22",
            "vds.example;calc",
            "",
            overlong.as_str(),
        ] {
            assert_eq!(
                scan_code(hostile, 22),
                Some("invalid_server_address"),
                "{hostile:?}"
            );
        }
        assert_eq!(scan_code("vds.example", 0), Some("invalid_server_address"));
    }

    #[test]
    fn errors_are_mapped_to_safe_codes_without_internal_detail() {
        assert_eq!(
            map_error(SshError::InvalidHostPin).code,
            "invalid_server_address"
        );
        assert_eq!(map_error(SshError::LaunchFailed).code, "ssh_client_missing");
        for error in [
            SshError::HostKeyUnavailable,
            SshError::TimedOut,
            SshError::LocalIo,
        ] {
            assert_eq!(map_error(error).code, "host_key_unavailable");
        }
        let failure = HostKeyFailure::unreachable();
        assert!(
            !failure.message.contains("ssh"),
            "no transport detail is exposed"
        );
    }
}
