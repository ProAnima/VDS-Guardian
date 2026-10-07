use crate::capture::create_hardened_destination;
use crate::{SshIdentity, SystemOpenSsh};
use std::{path::PathBuf, process::Command};

#[test]
fn known_hosts_paths_are_quoted_and_unquotable_paths_are_refused() {
    use std::path::Path;
    assert_eq!(
        crate::known_hosts_option(Path::new("C:/Users/Jane Doe/known hosts")),
        "UserKnownHostsFile=\"C:/Users/Jane Doe/known hosts\""
    );
    assert!(crate::usable_known_hosts_path(Path::new(
        "C:/Users/Jane Doe/AppData/kh"
    )));
    for bad in ["C:/a\"b", "C:/100%/kh", "/tmp/a\nb"] {
        assert!(!crate::usable_known_hosts_path(Path::new(bad)), "{bad:?}");
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
    let identity = SshIdentity::password_with_helper(b"S3cret-Pass!", PathBuf::from("the-helper"))?;
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
fn every_password_command_gets_its_own_one_time_token() -> Result<(), Box<dyn std::error::Error>> {
    let identity = SshIdentity::password_with_helper(b"S3cret-Pass!", PathBuf::from("the-helper"))?;
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
    let identity = SshIdentity::password_with_helper(b"S3cret-Pass!", PathBuf::from("the-helper"))?;
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
fn a_key_command_never_touches_the_askpass_environment() -> Result<(), Box<dyn std::error::Error>> {
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
