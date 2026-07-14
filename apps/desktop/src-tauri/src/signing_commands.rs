use guardian_os_keyring::OsCredentialStore;
use guardian_signing::{
    SigningIdentityEnrollment, SigningIdentityFailure, SigningIdentityManager,
    SigningIdentityStatus,
};
use std::path::PathBuf;
use tauri::Manager;

pub async fn status(
    app: tauri::AppHandle,
) -> Result<SigningIdentityStatus, SigningIdentityFailure> {
    let root = signing_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let manager = SigningIdentityManager::open(root).map_err(SigningIdentityFailure::from)?;
        manager
            .status(&OsCredentialStore)
            .map_err(SigningIdentityFailure::from)
    })
    .await
    .map_err(|_| SigningIdentityFailure::internal())?
}

pub async fn enroll(
    app: tauri::AppHandle,
) -> Result<SigningIdentityEnrollment, SigningIdentityFailure> {
    let root = signing_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let manager = SigningIdentityManager::open(root).map_err(SigningIdentityFailure::from)?;
        manager
            .enroll_or_load(&OsCredentialStore)
            .map(|identity| identity.enrollment())
            .map_err(SigningIdentityFailure::from)
    })
    .await
    .map_err(|_| SigningIdentityFailure::internal())?
}

fn signing_root(app: &tauri::AppHandle) -> Result<PathBuf, SigningIdentityFailure> {
    app.path()
        .app_config_dir()
        .map(|path| path.join("node"))
        .map_err(|_| SigningIdentityFailure::local_io())
}
