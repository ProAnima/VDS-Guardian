//! A password login must use password authentication and nothing else, must be
//! pinned exactly like a key login, and must never place the secret anywhere
//! OpenSSH, the operating system or a log could expose it.

mod common;

use guardian_ssh::{PinnedHost, SshIdentity, SshUser, SystemOpenSsh};
use std::path::Path;

fn pinned_host() -> Result<PinnedHost, Box<dyn std::error::Error>> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let mut blob = Vec::new();
    blob.extend_from_slice(&11_u32.to_be_bytes());
    blob.extend_from_slice(b"ssh-ed25519");
    blob.push(1);
    Ok(PinnedHost::parse(
        "vds.example",
        22,
        "ssh-ed25519",
        STANDARD.encode(blob),
    )?)
}

fn arguments(identity: &SshIdentity) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let plan = guardian_ssh::RemoteCapturePlan::from_roots(["/srv/app".to_owned()])?;
    Ok(SystemOpenSsh::default()
        .arguments(
            &pinned_host()?,
            &SshUser::parse("root")?,
            identity,
            Path::new("C:/known_hosts"),
            &plan,
        )
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect())
}

#[test]
fn a_password_login_enables_password_authentication_with_a_single_attempt()
-> Result<(), Box<dyn std::error::Error>> {
    let rendered = arguments(&common::password_identity()?)?.join(" ");
    for option in [
        "BatchMode=no",
        "PasswordAuthentication=yes",
        "KbdInteractiveAuthentication=no",
        "PubkeyAuthentication=no",
        "PreferredAuthentications=password",
        "NumberOfPasswordPrompts=1",
    ] {
        assert!(rendered.contains(option), "missing {option}");
    }
    Ok(())
}

#[test]
fn a_password_login_still_pins_the_host_and_never_passes_an_identity_file()
-> Result<(), Box<dyn std::error::Error>> {
    let rendered = arguments(&common::password_identity()?)?;
    let joined = rendered.join(" ");
    assert!(joined.contains("StrictHostKeyChecking=yes"));
    assert!(joined.contains("GlobalKnownHostsFile=none"));
    assert!(
        joined.contains("UserKnownHostsFile=\"C:/known_hosts\""),
        "quoted so a path with spaces stays one value"
    );
    assert!(!rendered.iter().any(|argument| argument == "-i"));
    assert!(!joined.contains("IdentitiesOnly"));
    assert!(joined.contains("root@vds.example"));
    Ok(())
}

#[test]
fn the_password_never_appears_in_any_argument() -> Result<(), Box<dyn std::error::Error>> {
    let rendered = arguments(&common::password_identity()?)?.join("\n");
    assert!(!rendered.contains(common::TEST_PASSWORD));
    assert!(!rendered.contains("S3cret"));
    Ok(())
}

#[test]
fn a_key_login_keeps_its_non_interactive_key_only_options() -> Result<(), Box<dyn std::error::Error>>
{
    let rendered = arguments(&common::key_identity()?)?;
    let joined = rendered.join(" ");
    for option in [
        "BatchMode=yes",
        "PasswordAuthentication=no",
        "PreferredAuthentications=publickey",
        "IdentitiesOnly=yes",
    ] {
        assert!(joined.contains(option), "missing {option}");
    }
    assert!(rendered.iter().any(|argument| argument == "-i"));
    assert!(!joined.contains("PasswordAuthentication=yes"));
    Ok(())
}

#[test]
fn only_a_login_password_marker_is_stored_for_a_valid_password()
-> Result<(), Box<dyn std::error::Error>> {
    let marker = SshIdentity::encode_password("pässw0rd 密码")?;
    assert!(marker.starts_with(b"PASSWORD-V1\n"));
    assert!(
        !String::from_utf8_lossy(&marker).contains("pässw0rd"),
        "the password is base64, not plaintext"
    );
    for bad in [
        "",
        "line\nbreak",
        "carriage\rreturn",
        "nul\0byte",
        &"x".repeat(257),
    ] {
        assert!(SshIdentity::encode_password(bad).is_err(), "{bad:?}");
    }
    assert!(SshIdentity::encode_password(&"x".repeat(256)).is_ok());
    Ok(())
}

#[test]
fn a_password_identity_is_recognised_as_a_password_not_a_key()
-> Result<(), Box<dyn std::error::Error>> {
    assert!(common::password_identity()?.is_password());
    assert!(!common::key_identity()?.is_password());
    Ok(())
}
