use guardian_core::{
    AuthKind, CredentialId, EnrollVerifiedProfileError, EnrollVerifiedProfileUseCase, HostPin,
    PreflightSshCaptureUseCase, ProfileId, ProfileStorePort, SecretValue, SshEndpoint, VdsProfile,
};
use guardian_os_keyring::OsCredentialStore;
use guardian_profile_store::ProfileStore;
use guardian_ssh::{
    PinnedHost, PinnedSshCapabilityProbe, SshIdentity, SshUser, SystemOpenSsh, openssh_fingerprint,
    password_logins_available,
};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::Manager;
use zeroize::Zeroize;

const MAX_KEY_BYTES: u64 = 64 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrollSshProfileRequest {
    label: String,
    host: String,
    port: u16,
    user: String,
    host_key: String,
    #[serde(default)]
    auth_kind: LoginMode,
    #[serde(default)]
    key_path: String,
    #[serde(default)]
    password: LoginPassword,
    /// The operator's explicit statement that the host key was verified out of band.
    #[serde(default)]
    host_key_confirmed: bool,
    /// When the key was fetched, the fingerprint the operator compared; it must be this key's.
    #[serde(default)]
    confirmed_fingerprint: Option<String>,
}

#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum LoginMode {
    #[default]
    Key,
    Password,
}

/// A login password received from the WebView: wiped when dropped and never printed.
#[derive(Default, Deserialize)]
#[serde(transparent)]
struct LoginPassword(String);

impl Drop for LoginPassword {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for LoginPassword {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("<redacted>")
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSummary {
    pub profile_id: String,
    pub label: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth_kind: AuthKind,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshPreflightSummary {
    pub tar_zstd: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileCommandFailure {
    pub code: &'static str,
    pub message: &'static str,
    pub remediation: &'static str,
}

pub async fn enroll(
    app: tauri::AppHandle,
    request: EnrollSshProfileRequest,
) -> Result<ProfileSummary, ProfileCommandFailure> {
    let root = profile_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || enroll_blocking(root, request))
        .await
        .map_err(|_| ProfileCommandFailure::internal())?
}

pub async fn list(app: tauri::AppHandle) -> Result<Vec<ProfileSummary>, ProfileCommandFailure> {
    let root = profile_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        ProfileStore::at(root)
            .list()
            .map(|profiles| profiles.iter().map(ProfileSummary::from).collect())
            .map_err(|_| ProfileCommandFailure::storage())
    })
    .await
    .map_err(|_| ProfileCommandFailure::internal())?
}

pub async fn test(app: tauri::AppHandle, profile_id: String) -> Result<(), ProfileCommandFailure> {
    let root = profile_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || test_blocking(root, profile_id))
        .await
        .map_err(|_| ProfileCommandFailure::internal())?
}

pub async fn preflight(
    app: tauri::AppHandle,
    profile_id: String,
) -> Result<SshPreflightSummary, ProfileCommandFailure> {
    let root = profile_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || preflight_blocking(root, profile_id))
        .await
        .map_err(|_| ProfileCommandFailure::internal())?
}

fn enroll_blocking(
    root: PathBuf,
    request: EnrollSshProfileRequest,
) -> Result<ProfileSummary, ProfileCommandFailure> {
    let (algorithm, public_key_base64) = split_host_key(&request.host_key)?;
    require_host_key_confirmation(&request, public_key_base64)?;
    let key = credential_secret(&request)?;
    let auth_kind = SshIdentity::auth_kind_of(key.expose())
        .map_err(|_| ProfileCommandFailure::invalid_key())?;
    let profile_id =
        ProfileId::parse(random_id("profile")).map_err(|_| ProfileCommandFailure::internal())?;
    let credential_id = CredentialId::parse(random_id("credential"))
        .map_err(|_| ProfileCommandFailure::internal())?;
    let profile = VdsProfile {
        profile_id,
        label: request.label,
        endpoint: SshEndpoint {
            host: request.host,
            port: request.port,
            user: request.user,
            host_pin: HostPin::parse(algorithm, public_key_base64)
                .map_err(|_| ProfileCommandFailure::invalid_profile())?,
        },
        auth_kind: Some(auth_kind),
        credential_id: credential_id.clone(),
    };
    profile
        .validate()
        .map_err(|_| ProfileCommandFailure::invalid_profile())?;
    let profiles = ProfileStore::at(root);
    let credentials = OsCredentialStore;
    let ssh = SystemOpenSsh::default();
    let probe = PinnedSshCapabilityProbe {
        ssh: &ssh,
        credentials: &credentials,
    };
    EnrollVerifiedProfileUseCase {
        profiles: &profiles,
        secrets: &credentials,
        probe: &probe,
    }
    .execute(profile.clone(), &key)
    .map_err(map_enrollment_error)?;
    Ok(ProfileSummary::from(&profile))
}

/// Trust in a host key is the operator's decision, so it is enforced here and not only by the
/// form: without the confirmation nothing is pinned, and a confirmed fingerprint must be the
/// fingerprint of exactly the key being pinned (ADR 0018).
fn require_host_key_confirmation(
    request: &EnrollSshProfileRequest,
    public_key_base64: &str,
) -> Result<(), ProfileCommandFailure> {
    if !request.host_key_confirmed {
        return Err(ProfileCommandFailure::host_key_unconfirmed());
    }
    match &request.confirmed_fingerprint {
        None => Ok(()),
        Some(confirmed) => openssh_fingerprint(public_key_base64)
            .ok()
            .filter(|actual| actual == confirmed)
            .map(|_| ())
            .ok_or_else(ProfileCommandFailure::host_key_mismatch),
    }
}

/// The bytes stored under the profile's credential id: a private key as-is, a `.pub` file as an
/// SSH-agent marker, or a login password as a `PASSWORD-V1` marker.
fn credential_secret(
    request: &EnrollSshProfileRequest,
) -> Result<SecretValue, ProfileCommandFailure> {
    match request.auth_kind {
        LoginMode::Key => {
            let key = read_key(Path::new(&request.key_path))?;
            SshIdentity::credential_from_key_file(key.expose())
                .map(SecretValue::new)
                .map_err(|_| ProfileCommandFailure::invalid_key())
        }
        LoginMode::Password => {
            if !password_logins_available() {
                return Err(ProfileCommandFailure::password_unavailable());
            }
            password_secret(&request.password.0)
        }
    }
}

fn password_secret(password: &str) -> Result<SecretValue, ProfileCommandFailure> {
    SshIdentity::encode_password(password)
        .map(SecretValue::new)
        .map_err(|_| ProfileCommandFailure::invalid_password())
}

fn map_enrollment_error(error: EnrollVerifiedProfileError) -> ProfileCommandFailure {
    match error {
        EnrollVerifiedProfileError::InvalidProfile => ProfileCommandFailure::invalid_profile(),
        EnrollVerifiedProfileError::Probe(_) | EnrollVerifiedProfileError::TarZstdUnsupported => {
            ProfileCommandFailure::preflight()
        }
        EnrollVerifiedProfileError::ProfileStore(_) => ProfileCommandFailure::storage(),
        EnrollVerifiedProfileError::CredentialExists
        | EnrollVerifiedProfileError::SecretStore(_)
        | EnrollVerifiedProfileError::Cleanup => ProfileCommandFailure::credential_store(),
    }
}

fn test_blocking(root: PathBuf, profile_id: String) -> Result<(), ProfileCommandFailure> {
    let id = ProfileId::parse(profile_id).map_err(|_| ProfileCommandFailure::not_found())?;
    let profile = ProfileStore::at(root)
        .get(&id)
        .map_err(|_| ProfileCommandFailure::storage())?
        .ok_or_else(ProfileCommandFailure::not_found)?;
    let host = PinnedHost::parse(
        &profile.endpoint.host,
        profile.endpoint.port,
        &profile.endpoint.host_pin.algorithm,
        &profile.endpoint.host_pin.public_key_base64,
    )
    .map_err(|_| ProfileCommandFailure::invalid_profile())?;
    let user = SshUser::parse(&profile.endpoint.user)
        .map_err(|_| ProfileCommandFailure::invalid_profile())?;
    let identity = SshIdentity::from_store(&OsCredentialStore, &profile.credential_id)
        .map_err(|_| ProfileCommandFailure::credential_store())?;
    SystemOpenSsh::default()
        .probe_connection(&host, &user, &identity)
        .map_err(|_| ProfileCommandFailure::connection())
}

fn preflight_blocking(
    root: PathBuf,
    profile_id: String,
) -> Result<SshPreflightSummary, ProfileCommandFailure> {
    let id = ProfileId::parse(profile_id).map_err(|_| ProfileCommandFailure::not_found())?;
    let profiles = ProfileStore::at(root);
    let ssh = SystemOpenSsh::default();
    let probe = PinnedSshCapabilityProbe {
        ssh: &ssh,
        credentials: &OsCredentialStore,
    };
    let capabilities = PreflightSshCaptureUseCase {
        profiles: &profiles,
        probe: &probe,
    }
    .execute(&id)
    .map_err(|_| ProfileCommandFailure::preflight())?;
    capabilities
        .tar_zstd
        .then_some(SshPreflightSummary { tar_zstd: true })
        .ok_or_else(ProfileCommandFailure::preflight)
}

fn split_host_key(value: &str) -> Result<(&str, &str), ProfileCommandFailure> {
    let mut values = value.split_ascii_whitespace();
    let algorithm = values
        .next()
        .ok_or_else(ProfileCommandFailure::invalid_profile)?;
    let key = values
        .next()
        .ok_or_else(ProfileCommandFailure::invalid_profile)?;
    values
        .next()
        .is_none()
        .then_some((algorithm, key))
        .ok_or_else(ProfileCommandFailure::invalid_profile)
}

fn read_key(path: &Path) -> Result<SecretValue, ProfileCommandFailure> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| ProfileCommandFailure::invalid_key_path())?;
    if !path.is_absolute()
        || !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_KEY_BYTES
    {
        return Err(ProfileCommandFailure::invalid_key_path());
    }
    fs::read(path)
        .map(SecretValue::new)
        .map_err(|_| ProfileCommandFailure::invalid_key_path())
}

fn random_id(prefix: &str) -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    let suffix = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{prefix}-{suffix}")
}

fn profile_root(app: &tauri::AppHandle) -> Result<PathBuf, ProfileCommandFailure> {
    app.path()
        .app_config_dir()
        .map(|path| path.join("profiles"))
        .map_err(|_| ProfileCommandFailure::storage())
}

impl From<&VdsProfile> for ProfileSummary {
    fn from(profile: &VdsProfile) -> Self {
        Self {
            profile_id: profile.profile_id.as_str().to_owned(),
            label: profile.label.clone(),
            host: profile.endpoint.host.clone(),
            port: profile.endpoint.port,
            user: profile.endpoint.user.clone(),
            auth_kind: profile.auth_kind.unwrap_or(AuthKind::SshKey),
        }
    }
}

impl ProfileCommandFailure {
    fn invalid_profile() -> Self {
        Self {
            code: "invalid_profile",
            message: "The SSH profile is invalid.",
            remediation: "Check the server name, SSH address, user, port, and verified host key.",
        }
    }
    fn invalid_key_path() -> Self {
        Self {
            code: "invalid_key_path",
            message: "The SSH key file is not a safe regular file.",
            remediation: "Use an absolute path to a non-symlink key file no larger than 64 KiB.",
        }
    }
    fn invalid_key() -> Self {
        Self {
            code: "invalid_ssh_key",
            message: "The SSH key file is not a supported key.",
            remediation: "Choose a dedicated unencrypted OpenSSH or PEM private key, or the .pub file of an ed25519 or ECDSA key that is loaded in your SSH agent.",
        }
    }
    fn host_key_unconfirmed() -> Self {
        Self {
            code: "host_key_unconfirmed",
            message: "The server's host key was not confirmed.",
            remediation: "Compare the host key or its fingerprint with a trusted source, then confirm it.",
        }
    }
    fn host_key_mismatch() -> Self {
        Self {
            code: "host_key_fingerprint_mismatch",
            message: "The confirmed fingerprint does not belong to this host key.",
            remediation: "Fetch the host key again and compare the fingerprint that is shown.",
        }
    }
    fn invalid_password() -> Self {
        Self {
            code: "invalid_login_password",
            message: "The login password cannot be used.",
            remediation: "Use 1 to 256 characters without line breaks.",
        }
    }
    fn password_unavailable() -> Self {
        Self {
            code: "password_login_unavailable",
            message: "Password logins are not available in this installation.",
            remediation: "Reinstall the application, or use an SSH key instead.",
        }
    }
    fn credential_store() -> Self {
        Self {
            code: "credential_store_unavailable",
            message: "The operating-system credential store could not save the SSH key.",
            remediation: "Unlock or configure the credential store and try again.",
        }
    }
    fn storage() -> Self {
        Self {
            code: "profile_storage_unavailable",
            message: "The server profile could not be saved.",
            remediation: "Check local application storage and try again.",
        }
    }
    fn not_found() -> Self {
        Self {
            code: "profile_not_found",
            message: "The server profile was not found.",
            remediation: "Refresh the server list and add the server again if needed.",
        }
    }
    fn connection() -> Self {
        Self {
            code: "ssh_connection_failed",
            message: "The server did not pass the pinned SSH connection check.",
            remediation: "Verify the address, SSH user, key authorization, and host key pin.",
        }
    }
    fn preflight() -> Self {
        Self {
            code: "ssh_preflight_failed",
            message: "The server is not ready for a verified archive capture.",
            remediation: "Install GNU tar with zstd support for the backup account and recheck SSH access.",
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
    use super::{
        EnrollSshProfileRequest, LoginMode, password_secret, require_host_key_confirmation,
    };

    const KEY: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIAgqcoUnCQ7+eoqxrmBwksjYpoBztPS3lYA3OqOZvZ+K";
    const KEY_FINGERPRINT: &str = "SHA256:tLrx5oHgmHF9+Rj+Pvc8PH0KLLP6y3wrk/Mc4bF8Mb8";

    fn confirmation(extra: &str) -> Result<EnrollSshProfileRequest, serde_json::Error> {
        request(&format!(
            r#"{{"label":"a","host":"h","port":22,"user":"u","hostKey":"ssh-ed25519 {KEY}"{extra}}}"#
        ))
    }

    #[test]
    fn nothing_is_pinned_without_the_operators_confirmation() -> Result<(), serde_json::Error> {
        let missing = require_host_key_confirmation(&confirmation("")?, KEY)
            .err()
            .map(|failure| failure.code);
        assert_eq!(missing, Some("host_key_unconfirmed"));
        let refused =
            require_host_key_confirmation(&confirmation(r#","hostKeyConfirmed":false"#)?, KEY);
        assert!(refused.is_err());
        assert!(
            require_host_key_confirmation(&confirmation(r#","hostKeyConfirmed":true"#)?, KEY)
                .is_ok()
        );
        Ok(())
    }

    #[test]
    fn a_confirmed_fingerprint_must_belong_to_the_pinned_key() -> Result<(), serde_json::Error> {
        let matching =
            format!(r#","hostKeyConfirmed":true,"confirmedFingerprint":"{KEY_FINGERPRINT}""#);
        assert!(require_host_key_confirmation(&confirmation(&matching)?, KEY).is_ok());
        let other =
            r#","hostKeyConfirmed":true,"confirmedFingerprint":"SHA256:somebody-elses-key""#;
        let failure = require_host_key_confirmation(&confirmation(other)?, KEY)
            .err()
            .map(|failure| failure.code);
        assert_eq!(failure, Some("host_key_fingerprint_mismatch"));
        Ok(())
    }

    fn request(json: &str) -> Result<EnrollSshProfileRequest, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn a_request_without_an_auth_kind_is_a_key_login() -> Result<(), serde_json::Error> {
        let parsed = request(
            r#"{"label":"a","host":"h","port":22,"user":"u","hostKey":"k","keyPath":"/k"}"#,
        )?;
        assert_eq!(parsed.auth_kind, LoginMode::Key);
        Ok(())
    }

    #[test]
    fn the_password_is_redacted_from_debug_output() -> Result<(), serde_json::Error> {
        let parsed = request(
            r#"{"label":"a","host":"h","port":22,"user":"root","hostKey":"k","authKind":"password","password":"S3cret-Pass!"}"#,
        )?;
        assert_eq!(parsed.auth_kind, LoginMode::Password);
        let rendered = format!("{parsed:?}");
        assert!(!rendered.contains("S3cret"), "{rendered}");
        assert!(rendered.contains("<redacted>"));
        Ok(())
    }

    #[test]
    fn a_usable_password_becomes_a_marker_and_an_unusable_one_is_rejected() {
        assert!(password_secret("pässw0rd").is_ok());
        for bad in ["", "two\nlines", "nul\0", &"x".repeat(257)] {
            let failure = password_secret(bad).err().map(|failure| failure.code);
            assert_eq!(failure, Some("invalid_login_password"), "{bad:?}");
        }
    }
}
