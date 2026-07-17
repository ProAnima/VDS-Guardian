use super::{RepositoryCommandFailure, RepositorySummary};
use guardian_configuration::{RepositoryRegistration, RepositoryStore};
use guardian_core::RepositoryId;
use guardian_local_repository::{
    LocalRepository, RecoveryBundleError, export_recovery_bundle, import_recovery_bundle,
};
use guardian_os_keyring::OsCredentialStore;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tauri::Manager;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRecoveryBundleRequest {
    repository_id: String,
    passphrase: String,
    passphrase_confirmation: String,
    output_path: String,
    confirmation: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRecoveryBundleRequest {
    repository_id: String,
    repository_path: String,
    input_path: String,
    passphrase: String,
    confirmation: String,
}

pub async fn export_recovery_bundle_file(
    app: tauri::AppHandle,
    request: ExportRecoveryBundleRequest,
) -> Result<(), RepositoryCommandFailure> {
    let root = config_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || export_bundle_blocking(root, request))
        .await
        .map_err(|_| RepositoryCommandFailure::internal())?
}

pub async fn import_recovery_bundle_file(
    app: tauri::AppHandle,
    request: ImportRecoveryBundleRequest,
) -> Result<RepositorySummary, RepositoryCommandFailure> {
    let root = config_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || import_bundle_blocking(root, request))
        .await
        .map_err(|_| RepositoryCommandFailure::internal())?
}

fn export_bundle_blocking(
    root: PathBuf,
    request: ExportRecoveryBundleRequest,
) -> Result<(), RepositoryCommandFailure> {
    validate_export_passphrases(&request.passphrase, &request.passphrase_confirmation)?;
    let repository_id = parse_repository_id(request.repository_id)?;
    let registration = RepositoryStore::at(root.join("repositories"))
        .get(&repository_id)
        .map_err(|_| RepositoryCommandFailure::storage())?
        .ok_or_else(RepositoryCommandFailure::invalid)?;
    let repository = open_repository(&registration.path, repository_id.clone())?;
    export_recovery_bundle(
        &repository,
        &OsCredentialStore,
        &repository_id,
        request.passphrase.as_bytes(),
        Path::new(&request.output_path),
        &request.confirmation,
    )
    .map_err(|error| map_bundle_error(error, BundleOperation::Export))
}

fn import_bundle_blocking(
    root: PathBuf,
    request: ImportRecoveryBundleRequest,
) -> Result<RepositorySummary, RepositoryCommandFailure> {
    require_passphrase(&request.passphrase)?;
    let repository_id = parse_repository_id(request.repository_id)?;
    let store = RepositoryStore::at(root.join("repositories"));
    let (repository, pending) =
        resolve_import_target(&store, &repository_id, Path::new(&request.repository_path))?;
    import_recovery_bundle(
        &repository,
        &OsCredentialStore,
        &repository_id,
        request.passphrase.as_bytes(),
        Path::new(&request.input_path),
        &request.confirmation,
    )
    .map_err(|error| map_bundle_error(error, BundleOperation::Import))?;
    finish_import(&store, &repository_id, pending)
}

fn resolve_import_target(
    store: &RepositoryStore,
    repository_id: &RepositoryId,
    requested_path: &Path,
) -> Result<(LocalRepository, Option<RepositoryRegistration>), RepositoryCommandFailure> {
    if let Some(existing) = store
        .get(repository_id)
        .map_err(|_| RepositoryCommandFailure::storage())?
    {
        return Ok((
            open_repository(&existing.path, repository_id.clone())?,
            None,
        ));
    }
    let repository = open_repository(requested_path, repository_id.clone())?;
    let registration = RepositoryRegistration::new(
        repository_id.clone(),
        format!("Recovered {}", repository_id.as_str()),
        repository.root().to_owned(),
    )
    .map_err(|_| RepositoryCommandFailure::invalid())?;
    Ok((repository, Some(registration)))
}

fn finish_import(
    store: &RepositoryStore,
    repository_id: &RepositoryId,
    pending: Option<RepositoryRegistration>,
) -> Result<RepositorySummary, RepositoryCommandFailure> {
    if let Some(registration) = pending {
        store
            .upsert(registration.clone())
            .map_err(|_| RepositoryCommandFailure::storage())?;
        return Ok(ready_summary(&registration));
    }
    store
        .get(repository_id)
        .map_err(|_| RepositoryCommandFailure::storage())?
        .map(|registration| ready_summary(&registration))
        .ok_or_else(RepositoryCommandFailure::storage)
}

fn ready_summary(registration: &RepositoryRegistration) -> RepositorySummary {
    let mut summary = RepositorySummary::from(registration);
    summary.recovery_ready = true;
    summary
}

fn validate_export_passphrases(
    passphrase: &str,
    confirmation: &str,
) -> Result<(), RepositoryCommandFailure> {
    require_passphrase(passphrase)?;
    if passphrase != confirmation {
        return Err(RepositoryCommandFailure::passphrase_mismatch());
    }
    Ok(())
}

fn require_passphrase(passphrase: &str) -> Result<(), RepositoryCommandFailure> {
    if passphrase.is_empty() {
        return Err(RepositoryCommandFailure::passphrase());
    }
    Ok(())
}

fn config_root(app: &tauri::AppHandle) -> Result<PathBuf, RepositoryCommandFailure> {
    app.path()
        .app_config_dir()
        .map_err(|_| RepositoryCommandFailure::storage())
}

fn parse_repository_id(value: String) -> Result<RepositoryId, RepositoryCommandFailure> {
    RepositoryId::parse(value).map_err(|_| RepositoryCommandFailure::invalid())
}

fn open_repository(
    path: &Path,
    id: RepositoryId,
) -> Result<LocalRepository, RepositoryCommandFailure> {
    LocalRepository::open(path, id).map_err(|_| RepositoryCommandFailure::repository())
}

#[derive(Clone, Copy)]
enum BundleOperation {
    Export,
    Import,
}

fn map_bundle_error(
    error: RecoveryBundleError,
    operation: BundleOperation,
) -> RepositoryCommandFailure {
    match error {
        RecoveryBundleError::ConfirmationMismatch => confirmation_failure(operation),
        RecoveryBundleError::NotConfigured => RepositoryCommandFailure::recovery(),
        RecoveryBundleError::InvalidBundle | RecoveryBundleError::Crypto => {
            RepositoryCommandFailure::recovery()
        }
        RecoveryBundleError::Repository(_) | RecoveryBundleError::Io => {
            RepositoryCommandFailure::storage()
        }
    }
}

fn confirmation_failure(operation: BundleOperation) -> RepositoryCommandFailure {
    let message = match operation {
        BundleOperation::Export => "The recovery export confirmation does not match.",
        BundleOperation::Import => "The recovery import confirmation does not match.",
    };
    RepositoryCommandFailure {
        code: "recovery_confirmation_mismatch",
        message,
        remediation: "Type the exact confirmation phrase shown for this repository.",
    }
}

#[cfg(test)]
mod tests {
    use super::{BundleOperation, confirmation_failure, validate_export_passphrases};

    #[test]
    fn export_rejects_mismatched_passphrases() {
        let result = validate_export_passphrases("correct horse", "correct house");
        assert!(matches!(
            result,
            Err(failure) if failure.code == "recovery_passphrase_mismatch"
        ));
    }

    #[test]
    fn import_confirmation_error_names_the_import_operation() {
        let failure = confirmation_failure(BundleOperation::Import);
        assert!(failure.message.contains("import"));
    }
}
