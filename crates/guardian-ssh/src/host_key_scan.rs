//! Fetching a server's public host key so the operator can compare its fingerprint with the
//! one their provider shows, instead of pasting the whole key by hand.
//!
//! This is only a *retrieval*: it authenticates nothing and trusts nothing. A key becomes a
//! pin only when the operator explicitly confirms the fingerprint (desktop acknowledgement);
//! an attacker on the path during this one lookup could present their own key, which is why the
//! fingerprint must be compared out of band and why the lookup is not available to MCP.

use crate::{SshError, process, valid_host};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
};
use guardian_core::{CancellationHandle, HostPin};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

const MAX_OUTPUT_BYTES: u64 = 16 * 1024;
/// Most to least preferred: the strongest algorithm the server offers wins.
const PREFERENCE: [&str; 4] = [
    "ssh-ed25519",
    "ecdsa-sha2-nistp521",
    "ecdsa-sha2-nistp384",
    "ecdsa-sha2-nistp256",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedHostKey {
    pub algorithm: String,
    pub public_key_base64: String,
    /// The standard OpenSSH form (`SHA256:` + unpadded base64), as `ssh-keygen -l` prints it.
    pub fingerprint: String,
}

/// Standard OpenSSH SHA-256 fingerprint of a public key blob given as base64.
pub fn openssh_fingerprint(public_key_base64: &str) -> Result<String, SshError> {
    let blob = STANDARD
        .decode(public_key_base64.as_bytes())
        .map_err(|_| SshError::InvalidHostPin)?;
    Ok(format!(
        "SHA256:{}",
        STANDARD_NO_PAD.encode(Sha256::digest(blob))
    ))
}

/// Asks `host:port` for its public host key by letting system OpenSSH connect and record the key.
///
/// `ssh-keyscan` is not used: the Windows build cannot negotiate the post-quantum key exchange
/// that current OpenSSH servers prefer, whereas `ssh` records the host key into a throw-away
/// `known_hosts` file as soon as it is received, before any authentication. No credential is
/// presented; the connection ends when authentication has nothing to offer.
pub fn scan_host_key(host: &str, port: u16, timeout: Duration) -> Result<ScannedHostKey, SshError> {
    scan_host_key_with("ssh", host, port, timeout)
}

pub(crate) fn scan_host_key_with(
    program: &str,
    host: &str,
    port: u16,
    timeout: Duration,
) -> Result<ScannedHostKey, SshError> {
    if !valid_host(host) || port == 0 {
        return Err(SshError::InvalidHostPin);
    }
    // Only the path is kept: an open handle would stop OpenSSH from appending to the file on Windows.
    let known_hosts = tempfile::NamedTempFile::new()
        .map_err(|_| SshError::LocalIo)?
        .into_temp_path();
    if !crate::usable_known_hosts_path(&known_hosts) {
        return Err(SshError::LocalIo);
    }
    let child = Command::new(program)
        .args(scan_arguments(host, port, timeout, &known_hosts))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| SshError::LaunchFailed)?;
    // The exit status is deliberately ignored: with no credential, ssh always ends in a failed
    // authentication. What matters is whether the server's key reached the file.
    match process::wait_for_exit(
        child,
        timeout + Duration::from_secs(3),
        &CancellationHandle::new(),
    ) {
        Ok(_) => {}
        Err(error) => return Err(crate::map_wait_error(error)),
    }
    let mut recorded = String::new();
    std::fs::File::open(&known_hosts)
        .and_then(|file| file.take(MAX_OUTPUT_BYTES).read_to_string(&mut recorded))
        .map_err(|_| SshError::LocalIo)?;
    parse_scan_output(&recorded)
}

fn scan_arguments(host: &str, port: u16, timeout: Duration, known_hosts: &Path) -> Vec<String> {
    let options = [
        "BatchMode=yes".to_owned(),
        format!("ConnectTimeout={}", timeout.as_secs().max(1)),
        "StrictHostKeyChecking=no".to_owned(),
        crate::known_hosts_option(known_hosts),
        "GlobalKnownHostsFile=none".to_owned(),
        "HashKnownHosts=no".to_owned(),
        "CheckHostIP=no".to_owned(),
        "UpdateHostKeys=no".to_owned(),
        "ProxyCommand=none".to_owned(),
        "LogLevel=ERROR".to_owned(),
        format!("HostKeyAlgorithms={}", PREFERENCE.join(",")),
        "PubkeyAuthentication=no".to_owned(),
        "PasswordAuthentication=no".to_owned(),
        "KbdInteractiveAuthentication=no".to_owned(),
    ];
    let mut arguments = vec!["-F".to_owned(), "none".to_owned()];
    arguments.extend(
        options
            .into_iter()
            .flat_map(|option| ["-o".to_owned(), option]),
    );
    arguments.extend([
        "-p".to_owned(),
        port.to_string(),
        format!("hostkey-scan@{host}"),
        "exit".to_owned(),
    ]);
    arguments
}

/// Picks the most preferred valid key from the throwaway `known_hosts` lines (`<host> <algorithm> <base64>`).
pub(crate) fn parse_scan_output(output: &str) -> Result<ScannedHostKey, SshError> {
    let mut best: Option<(usize, ScannedHostKey)> = None;
    for line in output
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
    {
        let mut fields = line.split_ascii_whitespace();
        let (Some(_host), Some(algorithm), Some(key)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let Some(rank) = PREFERENCE
            .iter()
            .position(|candidate| *candidate == algorithm)
        else {
            continue;
        };
        if HostPin::parse(algorithm, key).is_err()
            || best.as_ref().is_some_and(|(current, _)| *current <= rank)
        {
            continue;
        }
        best = Some((
            rank,
            ScannedHostKey {
                algorithm: algorithm.to_owned(),
                public_key_base64: key.to_owned(),
                fingerprint: openssh_fingerprint(key)?,
            },
        ));
    }
    best.map(|(_, key)| key).ok_or(SshError::HostKeyUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ED25519: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIAgqcoUnCQ7+eoqxrmBwksjYpoBztPS3lYA3OqOZvZ+K";

    fn ecdsa_blob(algorithm: &str) -> String {
        let mut blob = Vec::new();
        blob.extend_from_slice(&u32::try_from(algorithm.len()).unwrap_or(0).to_be_bytes());
        blob.extend_from_slice(algorithm.as_bytes());
        blob.push(1);
        STANDARD.encode(blob)
    }

    #[test]
    fn the_fingerprint_matches_what_ssh_keygen_prints() -> Result<(), SshError> {
        assert_eq!(
            openssh_fingerprint(ED25519)?,
            "SHA256:tLrx5oHgmHF9+Rj+Pvc8PH0KLLP6y3wrk/Mc4bF8Mb8"
        );
        Ok(())
    }

    #[test]
    fn comments_and_unrelated_lines_are_ignored_and_the_key_is_parsed() -> Result<(), SshError> {
        let output =
            format!("# vds.example:22 SSH-2.0-OpenSSH_9.6\nvds.example ssh-ed25519 {ED25519}\n");
        let key = parse_scan_output(&output)?;
        assert_eq!(
            (key.algorithm.as_str(), key.public_key_base64.as_str()),
            ("ssh-ed25519", ED25519)
        );
        assert_eq!(
            key.fingerprint,
            "SHA256:tLrx5oHgmHF9+Rj+Pvc8PH0KLLP6y3wrk/Mc4bF8Mb8"
        );
        Ok(())
    }

    #[test]
    fn the_strongest_offered_algorithm_wins_regardless_of_order() -> Result<(), SshError> {
        let p256 = ecdsa_blob("ecdsa-sha2-nistp256");
        let output = format!("h ecdsa-sha2-nistp256 {p256}\nh ssh-ed25519 {ED25519}\n");
        assert_eq!(parse_scan_output(&output)?.algorithm, "ssh-ed25519");
        let only_ecdsa = format!("h ecdsa-sha2-nistp256 {p256}\n");
        assert_eq!(
            parse_scan_output(&only_ecdsa)?.algorithm,
            "ecdsa-sha2-nistp256"
        );
        Ok(())
    }

    #[test]
    fn missing_malformed_or_unsupported_keys_are_rejected() {
        let rsa = ecdsa_blob("ssh-rsa");
        for bad in [
            "",
            "# only a banner\n",
            "h ssh-ed25519\n",
            "h ssh-ed25519 not-base64!!\n",
            &format!("h ssh-rsa {rsa}\n"),
            &format!("h ssh-ed25519 {}\n", ecdsa_blob("ecdsa-sha2-nistp256")),
        ] {
            assert_eq!(
                parse_scan_output(bad),
                Err(SshError::HostKeyUnavailable),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn a_host_that_could_act_as_an_option_is_never_passed_to_the_program() {
        // Launching this missing program fails as `LaunchFailed`: `InvalidHostPin` means unlaunched.
        let scan = |host: &str, port| {
            scan_host_key_with("guardian-no-such-ssh", host, port, Duration::from_secs(1))
        };
        for bad in [
            "-oProxyCommand=calc",
            "",
            "host name",
            "host\n-oX=y",
            "\thost",
            "host%h",
            "host;rm",
            "a..b",
            ".leading",
        ] {
            assert_eq!(scan(bad, 22), Err(SshError::InvalidHostPin), "{bad:?}");
        }
        assert_eq!(scan("vds.example", 0), Err(SshError::InvalidHostPin));
        assert_eq!(scan(&"a".repeat(254), 22), Err(SshError::InvalidHostPin));
        assert_eq!(scan("vds.example", 22), Err(SshError::LaunchFailed));
    }

    #[test]
    fn arguments_are_direct_argv_pin_nothing_and_offer_no_credential() {
        let arguments = scan_arguments(
            "vds.example",
            2222,
            Duration::from_secs(8),
            Path::new("C:/scan/known_hosts"),
        );
        let joined = arguments.join(" ");
        assert_eq!(arguments.first().map(String::as_str), Some("-F"));
        assert!(
            joined.contains("StrictHostKeyChecking=no"),
            "records the key without prompting"
        );
        assert!(joined.contains("UserKnownHostsFile=\"C:/scan/known_hosts\""));
        assert!(joined.contains("GlobalKnownHostsFile=none"));
        assert!(joined.contains("ProxyCommand=none"));
        for off in [
            "PubkeyAuthentication=no",
            "PasswordAuthentication=no",
            "KbdInteractiveAuthentication=no",
            "BatchMode=yes",
        ] {
            assert!(joined.contains(off), "missing {off}");
        }
        assert!(joined.contains("HostKeyAlgorithms=ssh-ed25519,ecdsa-sha2-nistp521"));
        assert_eq!(
            &arguments[arguments.len() - 4..],
            ["-p", "2222", "hostkey-scan@vds.example", "exit"]
        );
        assert!(
            !arguments
                .iter()
                .any(|argument| argument.contains("IdentityFile") || argument == "-i")
        );
    }
}
