use super::{
    EnrollSshProfileRequest, LoginMode, ProfileSummary, credential_secret, login_credential,
    new_profile, password_secret, require_host_key_confirmation,
};
use guardian_core::{AuthKind, HostPin};
use guardian_ssh::password_logins_available;
use std::path::Path;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const KEY: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIAgqcoUnCQ7+eoqxrmBwksjYpoBztPS3lYA3OqOZvZ+K";
const KEY_FINGERPRINT: &str = "SHA256:tLrx5oHgmHF9+Rj+Pvc8PH0KLLP6y3wrk/Mc4bF8Mb8";
/// A structurally valid PEM EC private key (not a usable key; only its kind is read here).
const PEM_PRIVATE_KEY: &str =
    "-----BEGIN EC PRIVATE KEY-----\nMAkCAQACBAECAwQ=\n-----END EC PRIVATE KEY-----\n";

fn request(json: &str) -> Result<EnrollSshProfileRequest, serde_json::Error> {
    serde_json::from_str(json)
}

fn confirmation(extra: &str) -> Result<EnrollSshProfileRequest, serde_json::Error> {
    request(&format!(
        r#"{{"label":"a","host":"h","port":22,"user":"u","hostKey":"ssh-ed25519 {KEY}"{extra}}}"#
    ))
}

fn password_login(password: &str) -> Result<EnrollSshProfileRequest, serde_json::Error> {
    let password = serde_json::to_string(password)?;
    confirmation(&format!(r#","authKind":"password","password":{password}"#))
}

fn key_login(path: &Path) -> Result<EnrollSshProfileRequest, serde_json::Error> {
    let path = serde_json::to_string(&path.display().to_string())?;
    confirmation(&format!(r#","authKind":"key","keyPath":{path}"#))
}

fn failure_code<T>(result: Result<T, super::ProfileCommandFailure>) -> Option<&'static str> {
    result.err().map(|failure| failure.code)
}

/// Turns a command failure into a test error that names its code.
fn ok<T>(result: Result<T, super::ProfileCommandFailure>) -> Result<T, String> {
    result.map_err(|failure| failure.code.to_owned())
}

#[test]
fn nothing_is_pinned_without_the_operators_confirmation() -> Result<(), serde_json::Error> {
    let missing = require_host_key_confirmation(&confirmation("")?, KEY);
    assert_eq!(failure_code(missing), Some("host_key_unconfirmed"));
    let refused =
        require_host_key_confirmation(&confirmation(r#","hostKeyConfirmed":false"#)?, KEY);
    assert_eq!(failure_code(refused), Some("host_key_unconfirmed"));
    assert!(
        require_host_key_confirmation(&confirmation(r#","hostKeyConfirmed":true"#)?, KEY).is_ok()
    );
    Ok(())
}

#[test]
fn a_confirmed_fingerprint_must_belong_to_the_pinned_key() -> Result<(), serde_json::Error> {
    let matching =
        format!(r#","hostKeyConfirmed":true,"confirmedFingerprint":"{KEY_FINGERPRINT}""#);
    assert!(require_host_key_confirmation(&confirmation(&matching)?, KEY).is_ok());
    for other in ["SHA256:somebody-elses-key", "", "sha256:tLrx5oHgmHF9"] {
        let extra = format!(r#","hostKeyConfirmed":true,"confirmedFingerprint":"{other}""#);
        let failure = require_host_key_confirmation(&confirmation(&extra)?, KEY);
        assert_eq!(
            failure_code(failure),
            Some("host_key_fingerprint_mismatch"),
            "{other:?}"
        );
    }
    Ok(())
}

#[test]
fn a_request_without_an_auth_kind_is_a_key_login() -> Result<(), serde_json::Error> {
    let parsed =
        request(r#"{"label":"a","host":"h","port":22,"user":"u","hostKey":"k","keyPath":"/k"}"#)?;
    assert_eq!(parsed.auth_kind, LoginMode::Key);
    Ok(())
}

#[test]
fn the_password_is_redacted_from_debug_output() -> Result<(), serde_json::Error> {
    let parsed = password_login("S3cret-Pass!")?;
    assert_eq!(parsed.auth_kind, LoginMode::Password);
    let rendered = format!("{parsed:?}");
    assert!(!rendered.contains("S3cret"), "{rendered}");
    assert!(rendered.contains("<redacted>"));
    Ok(())
}

#[test]
fn a_usable_password_becomes_a_marker_and_an_unusable_one_is_rejected() {
    assert!(password_secret("pässw0rd").is_ok());
    for bad in ["", "two\nlines", "cr\rreturn", "nul\0", &"x".repeat(257)] {
        assert_eq!(
            failure_code(password_secret(bad)),
            Some("invalid_login_password"),
            "{bad:?}"
        );
    }
}

#[test]
fn a_password_login_is_refused_while_no_askpass_helper_is_registered() -> TestResult {
    // The test binary never registers a helper, exactly like a build that lost its askpass entry.
    assert!(!password_logins_available());
    let request = password_login("S3cret-Pass!")?;
    let code = failure_code(credential_secret(&request, password_logins_available()));
    assert_eq!(code, Some("password_login_unavailable"));
    Ok(())
}

#[test]
fn an_unusable_password_is_rejected_even_when_password_logins_are_available() -> TestResult {
    for bad in ["", "nul\0byte", &"x".repeat(257)] {
        let code = failure_code(login_credential(&password_login(bad)?, true));
        assert_eq!(code, Some("invalid_login_password"), "{bad:?}");
    }
    Ok(())
}

#[test]
fn the_login_kind_is_recorded_from_the_stored_credential() -> TestResult {
    let (_, kind) = ok(login_credential(&password_login("S3cret-Pass!")?, true))?;
    assert_eq!(kind, AuthKind::Password);
    let directory = tempfile::tempdir()?;
    let private_key = directory.path().join("id_ecdsa");
    std::fs::write(&private_key, PEM_PRIVATE_KEY)?;
    let (_, kind) = ok(login_credential(&key_login(&private_key)?, false))?;
    assert_eq!(kind, AuthKind::SshKey);
    let agent_key = directory.path().join("id_ed25519.pub");
    std::fs::write(&agent_key, format!("ssh-ed25519 {KEY} operator@laptop\n"))?;
    let (_, kind) = ok(login_credential(&key_login(&agent_key)?, false))?;
    assert_eq!(kind, AuthKind::SshAgent);
    Ok(())
}

#[test]
fn the_enrolled_profile_and_its_summary_carry_the_recorded_kind() -> TestResult {
    let pin = HostPin::parse("ssh-ed25519", KEY)?;
    let profile = ok(new_profile(
        password_login("S3cret-Pass!")?,
        pin,
        AuthKind::Password,
    ))?;
    assert_eq!(profile.auth_kind, Some(AuthKind::Password));
    let summary = serde_json::to_value(ProfileSummary::from(&profile))?;
    assert_eq!(summary["authKind"], "password");
    assert!(!summary.to_string().contains("S3cret"));
    Ok(())
}

#[test]
fn a_profile_saved_before_the_kind_was_recorded_is_not_shown_as_a_key_login() -> TestResult {
    let pin = HostPin::parse("ssh-ed25519", KEY)?;
    let mut legacy = ok(new_profile(confirmation("")?, pin, AuthKind::SshKey))?;
    legacy.auth_kind = None;
    let summary = ProfileSummary::from(&legacy);
    assert_eq!(summary.auth_kind, None);
    let rendered = serde_json::to_value(&summary)?;
    assert!(rendered.get("authKind").is_none(), "{rendered}");
    Ok(())
}
