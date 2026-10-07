use crate::ids::random_id;
use guardian_core::{
    AuthKind, CredentialId, EnrollVerifiedProfileError, EnrollVerifiedProfileUseCase, HostPin,
    ProfileId, SecretValue, SshEndpoint, VdsProfile,
};
use guardian_os_keyring::OsCredentialStore;
use guardian_profile_store::ProfileStore;
use guardian_ssh::{
    PinnedSshCapabilityProbe, SshIdentity, SystemOpenSsh, openssh_fingerprint,
    password_logins_available,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::Manager;
use zeroize::Zeroize;

mod failure;
#[cfg(test)]
mod tests;

pub use failure::ProfileCommandFailure;

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
    /// Absent when the profile was saved before the login kind was recorded; never guessed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_kind: Option<AuthKind>,
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

fn enroll_blocking(
    root: PathBuf,
    request: EnrollSshProfileRequest,
) -> Result<ProfileSummary, ProfileCommandFailure> {
    let (algorithm, public_key_base64) = split_host_key(&request.host_key)?;
    require_host_key_confirmation(&request, public_key_base64)?;
    let host_pin = HostPin::parse(algorithm, public_key_base64)
        .map_err(|_| ProfileCommandFailure::invalid_profile())?;
    let (key, auth_kind) = login_credential(&request, password_logins_available())?;
    let profile = new_profile(request, host_pin, auth_kind)?;
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

/// The profile to enroll, with fresh ids and the login kind recorded for display.
fn new_profile(
    request: EnrollSshProfileRequest,
    host_pin: HostPin,
    auth_kind: AuthKind,
) -> Result<VdsProfile, ProfileCommandFailure> {
    let profile = VdsProfile {
        profile_id: ProfileId::parse(random_id("profile"))
            .map_err(|_| ProfileCommandFailure::internal())?,
        label: request.label,
        endpoint: SshEndpoint {
            host: request.host,
            port: request.port,
            user: request.user,
            host_pin,
        },
        auth_kind: Some(auth_kind),
        credential_id: CredentialId::parse(random_id("credential"))
            .map_err(|_| ProfileCommandFailure::internal())?,
    };
    profile
        .validate()
        .map_err(|_| ProfileCommandFailure::invalid_profile())?;
    Ok(profile)
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

/// The stored credential together with the kind it is recorded as. The kind is read back from
/// the credential itself, so the record cannot disagree with what is stored.
fn login_credential(
    request: &EnrollSshProfileRequest,
    password_logins: bool,
) -> Result<(SecretValue, AuthKind), ProfileCommandFailure> {
    let secret = credential_secret(request, password_logins)?;
    let kind = SshIdentity::auth_kind_of(secret.expose())
        .map_err(|_| ProfileCommandFailure::invalid_key())?;
    Ok((secret, kind))
}

/// The bytes stored under the profile's credential id: a private key as-is, a `.pub` file as an
/// SSH-agent marker, or a login password as a `PASSWORD-V1` marker.
fn credential_secret(
    request: &EnrollSshProfileRequest,
    password_logins: bool,
) -> Result<SecretValue, ProfileCommandFailure> {
    match request.auth_kind {
        LoginMode::Key => {
            let key = read_key(Path::new(&request.key_path))?;
            SshIdentity::credential_from_key_file(key.expose())
                .map(SecretValue::new)
                .map_err(|_| ProfileCommandFailure::invalid_key())
        }
        LoginMode::Password if !password_logins => {
            Err(ProfileCommandFailure::password_unavailable())
        }
        LoginMode::Password => password_secret(&request.password.0),
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
            auth_kind: profile.auth_kind,
        }
    }
}
