use guardian_core::{
    BrowseRemoteDirectoryError, BrowseRemoteDirectoryUseCase, ProfileId, RemoteBrowsePage,
    RemoteBrowseRequest, RemotePath,
};
use guardian_os_keyring::OsCredentialStore;
use guardian_profile_store::ProfileStore;
use guardian_ssh::{SshRemoteBrowserAdapter, SystemOpenSsh};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};
use tauri::Manager;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseDirectoryRequest {
    profile_id: String,
    directory: String,
    cursor: Option<String>,
    limit: u16,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseDirectoryFailure {
    pub code: &'static str,
    pub message: &'static str,
    pub remediation: &'static str,
}

pub async fn browse(
    app: tauri::AppHandle,
    request: BrowseDirectoryRequest,
) -> Result<RemoteBrowsePage, BrowseDirectoryFailure> {
    let root = app
        .path()
        .app_config_dir()
        .map_err(|_| BrowseDirectoryFailure::storage())?;
    tauri::async_runtime::spawn_blocking(move || browse_blocking(root, request))
        .await
        .map_err(|_| BrowseDirectoryFailure::internal())?
}

fn browse_blocking(
    root: PathBuf,
    request: BrowseDirectoryRequest,
) -> Result<RemoteBrowsePage, BrowseDirectoryFailure> {
    let profile_id =
        ProfileId::parse(request.profile_id).map_err(|_| BrowseDirectoryFailure::invalid())?;
    let request = RemoteBrowseRequest {
        directory: RemotePath::parse(request.directory)
            .map_err(|_| BrowseDirectoryFailure::invalid())?,
        cursor: request.cursor,
        limit: request.limit,
    };
    let profiles = ProfileStore::at(root.join("profiles"));
    let ssh = SystemOpenSsh::default()
        .with_total_timeout(Duration::from_secs(30))
        .with_idle_timeout(Duration::from_secs(15));
    let browser = SshRemoteBrowserAdapter {
        ssh: &ssh,
        credentials: &OsCredentialStore,
    };
    BrowseRemoteDirectoryUseCase {
        profiles: &profiles,
        browser: &browser,
    }
    .execute(&profile_id, &request)
    .map_err(map_browse_error)
}

fn map_browse_error(error: BrowseRemoteDirectoryError) -> BrowseDirectoryFailure {
    match error {
        BrowseRemoteDirectoryError::ProfileNotFound => BrowseDirectoryFailure::not_found(),
        BrowseRemoteDirectoryError::ProfileStore(_) => BrowseDirectoryFailure::storage(),
        BrowseRemoteDirectoryError::Browser(_) => BrowseDirectoryFailure::unavailable(),
        BrowseRemoteDirectoryError::InvalidRequest(_)
        | BrowseRemoteDirectoryError::InvalidPage(_) => BrowseDirectoryFailure::invalid(),
    }
}

impl BrowseDirectoryFailure {
    fn invalid() -> Self {
        Self {
            code: "invalid_remote_directory",
            message: "The requested server directory or its listing was rejected.",
            remediation: "Choose a safe absolute Linux path and refresh the directory.",
        }
    }
    fn not_found() -> Self {
        Self {
            code: "profile_not_found",
            message: "The selected server was not found.",
            remediation: "Refresh the server list and select an enrolled server.",
        }
    }
    fn unavailable() -> Self {
        Self {
            code: "remote_browser_unavailable",
            message: "The server directory could not be read safely.",
            remediation: "Check SSH access and read permission for this directory, then retry.",
        }
    }
    fn storage() -> Self {
        Self {
            code: "profile_storage_unavailable",
            message: "Local server configuration is unavailable.",
            remediation: "Check local application storage and try again.",
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
