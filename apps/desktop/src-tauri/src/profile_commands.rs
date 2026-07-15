use guardian_core::{
    CredentialId, EnrollProfileUseCase, HostPin, PreflightSshCaptureUseCase, ProfileId,
    ProfileStorePort, SecretStore, SecretValue, SshEndpoint, VdsProfile,
};
use guardian_os_keyring::OsCredentialStore;
use guardian_profile_store::ProfileStore;
use guardian_ssh::{PinnedHost, PinnedSshCapabilityProbe, SshIdentity, SshUser, SystemOpenSsh};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::Manager;

const MAX_KEY_BYTES: u64 = 64 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrollSshProfileRequest {
    label: String,
    host: String,
    port: u16,
    user: String,
    host_key: String,
    key_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSummary {
    pub profile_id: String,
    pub label: String,
    pub host: String,
    pub port: u16,
    pub user: String,
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
        credential_id: credential_id.clone(),
    };
    profile
        .validate()
        .map_err(|_| ProfileCommandFailure::invalid_profile())?;
    let key = read_key(Path::new(&request.key_path))?;
    SshIdentity::validate(key.expose()).map_err(|_| ProfileCommandFailure::invalid_key())?;
    let store = OsCredentialStore;
    if store
        .load(&credential_id)
        .map_err(|_| ProfileCommandFailure::credential_store())?
        .is_some()
    {
        return Err(ProfileCommandFailure::credential_store());
    }
    store
        .store(&credential_id, &key)
        .map_err(|_| ProfileCommandFailure::credential_store())?;
    let stored = store
        .load(&credential_id)
        .map_err(|_| ProfileCommandFailure::credential_store())?
        .ok_or_else(ProfileCommandFailure::credential_store)?;
    SshIdentity::validate(stored.expose())
        .map_err(|_| ProfileCommandFailure::credential_store())?;
    EnrollProfileUseCase {
        store: &ProfileStore::at(root),
    }
    .execute(profile.clone())
    .map_err(|_| ProfileCommandFailure::storage())?;
    Ok(ProfileSummary::from(&profile))
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
        .probe_connection(&host, &user, identity.path())
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
            message: "The SSH key is not a supported unencrypted private key.",
            remediation: "Use a dedicated unencrypted OpenSSH or PEM private key protected by the operating-system credential store.",
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
