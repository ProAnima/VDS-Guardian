use super::{RepositoryCommandFailure, RepositorySummary, summarize_registration};
use guardian_configuration::{CapturePlanStore, RepositoryStore};
use guardian_core::RepositoryId;
use guardian_local_repository::LocalRepository;
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::Manager;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRepositoryPathRequest {
    repository_id: String,
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRepositoryRequest {
    repository_id: String,
    confirmed: bool,
}

pub async fn update_path(
    app: tauri::AppHandle,
    request: UpdateRepositoryPathRequest,
) -> Result<RepositorySummary, RepositoryCommandFailure> {
    let root = app
        .path()
        .app_config_dir()
        .map_err(|_| RepositoryCommandFailure::storage())?;
    tauri::async_runtime::spawn_blocking(move || update_path_blocking(root, request))
        .await
        .map_err(|_| RepositoryCommandFailure::internal())?
}

pub async fn delete(
    app: tauri::AppHandle,
    request: DeleteRepositoryRequest,
) -> Result<(), RepositoryCommandFailure> {
    let root = app
        .path()
        .app_config_dir()
        .map_err(|_| RepositoryCommandFailure::storage())?;
    tauri::async_runtime::spawn_blocking(move || delete_blocking(root, request))
        .await
        .map_err(|_| RepositoryCommandFailure::internal())?
}

fn update_path_blocking(
    root: PathBuf,
    request: UpdateRepositoryPathRequest,
) -> Result<RepositorySummary, RepositoryCommandFailure> {
    let id = parse_id(request.repository_id)?;
    let store = RepositoryStore::at(root.join("repositories"));
    let path = validate_existing_repository(Path::new(&request.path))?;
    let repository = LocalRepository::open(&path, id.clone())
        .map_err(|_| RepositoryCommandFailure::existing_repository_required())?;
    store
        .update_path(&id, repository.root().to_owned())
        .map_err(|_| RepositoryCommandFailure::storage())?
        .ok_or_else(RepositoryCommandFailure::not_found)
        .map(|updated| summarize_registration(&updated))
}

fn delete_blocking(
    root: PathBuf,
    request: DeleteRepositoryRequest,
) -> Result<(), RepositoryCommandFailure> {
    if !request.confirmed {
        return Err(RepositoryCommandFailure::removal_not_confirmed());
    }
    let id = parse_id(request.repository_id)?;
    let plans = CapturePlanStore::at(root.join("plans"))
        .list()
        .map_err(|_| RepositoryCommandFailure::storage())?;
    if plans.iter().any(|stored| stored.plan.repository_id == id) {
        return Err(RepositoryCommandFailure::in_use());
    }
    RepositoryStore::at(root.join("repositories"))
        .remove(&id)
        .map_err(|_| RepositoryCommandFailure::storage())?
        .ok_or_else(RepositoryCommandFailure::not_found)?;
    Ok(())
}

fn parse_id(value: String) -> Result<RepositoryId, RepositoryCommandFailure> {
    RepositoryId::parse(value).map_err(|_| RepositoryCommandFailure::invalid())
}

fn validate_existing_repository(path: &Path) -> Result<PathBuf, RepositoryCommandFailure> {
    let root_metadata = fs::symlink_metadata(path)
        .map_err(|_| RepositoryCommandFailure::existing_repository_required())?;
    let metadata_path = path.join("repository.json");
    let repository_metadata = fs::symlink_metadata(metadata_path)
        .map_err(|_| RepositoryCommandFailure::existing_repository_required())?;
    if !path.is_absolute()
        || !root_metadata.is_dir()
        || root_metadata.file_type().is_symlink()
        || !repository_metadata.is_file()
        || repository_metadata.file_type().is_symlink()
    {
        return Err(RepositoryCommandFailure::existing_repository_required());
    }
    fs::canonicalize(path).map_err(|_| RepositoryCommandFailure::existing_repository_required())
}

#[cfg(test)]
mod tests {
    use super::*;
    use guardian_configuration::RepositoryRegistration;

    #[test]
    fn path_update_requires_the_same_existing_repository() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = tempfile::tempdir()?;
        let old = tempfile::tempdir()?;
        let replacement = tempfile::tempdir()?;
        let other = tempfile::tempdir()?;
        let id = RepositoryId::parse("repository-001")?;
        LocalRepository::open(old.path(), id.clone())?;
        LocalRepository::open(replacement.path(), id.clone())?;
        LocalRepository::open(other.path(), RepositoryId::parse("repository-002")?)?;
        let store = RepositoryStore::at(root.path().join("repositories"));
        store.upsert(RepositoryRegistration::new(
            id.clone(),
            "Backups".to_owned(),
            fs::canonicalize(old.path())?,
        )?)?;

        let updated = update_path_blocking(
            root.path().to_owned(),
            update_request(&id, replacement.path()),
        )
        .map_err(|failure| failure.code)?;
        assert_eq!(
            PathBuf::from(updated.path),
            fs::canonicalize(replacement.path())?
        );
        let rejected =
            update_path_blocking(root.path().to_owned(), update_request(&id, other.path()));
        assert_failure_code(rejected, "existing_repository_required")?;
        Ok(())
    }

    #[test]
    fn removal_requires_confirmation_and_preserves_backup_files()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let location = tempfile::tempdir()?;
        let id = RepositoryId::parse("repository-001")?;
        LocalRepository::open(location.path(), id.clone())?;
        RepositoryStore::at(root.path().join("repositories")).upsert(
            RepositoryRegistration::new(
                id.clone(),
                "Backups".to_owned(),
                fs::canonicalize(location.path())?,
            )?,
        )?;
        let unconfirmed = DeleteRepositoryRequest {
            repository_id: id.as_str().to_owned(),
            confirmed: false,
        };
        assert_failure_code(
            delete_blocking(root.path().to_owned(), unconfirmed),
            "repository_removal_not_confirmed",
        )?;

        let confirmed = DeleteRepositoryRequest {
            repository_id: id.as_str().to_owned(),
            confirmed: true,
        };
        delete_blocking(root.path().to_owned(), confirmed).map_err(|failure| failure.code)?;
        assert!(
            RepositoryStore::at(root.path().join("repositories"))
                .get(&id)?
                .is_none()
        );
        assert!(location.path().join("repository.json").is_file());
        Ok(())
    }

    #[test]
    fn removal_is_blocked_while_a_saved_plan_uses_the_repository()
    -> Result<(), Box<dyn std::error::Error>> {
        use guardian_configuration::StoredCapturePlan;
        use guardian_core::{FilesystemCapturePlan, PlanId, ProfileId};
        let root = tempfile::tempdir()?;
        let location = tempfile::tempdir()?;
        let id = RepositoryId::parse("repository-001")?;
        LocalRepository::open(location.path(), id.clone())?;
        RepositoryStore::at(root.path().join("repositories")).upsert(
            RepositoryRegistration::new(
                id.clone(),
                "Backups".to_owned(),
                fs::canonicalize(location.path())?,
            )?,
        )?;
        let plan = FilesystemCapturePlan {
            plan_id: PlanId::parse("plan-001")?,
            version: 1,
            profile_id: ProfileId::parse("profile-001")?,
            repository_id: id.clone(),
            roots: vec!["/srv/app".to_owned()],
            database_path: None,
        };
        CapturePlanStore::at(root.path().join("plans")).upsert(StoredCapturePlan::new(plan)?)?;

        let request = DeleteRepositoryRequest {
            repository_id: id.as_str().to_owned(),
            confirmed: true,
        };
        assert_failure_code(
            delete_blocking(root.path().to_owned(), request),
            "repository_in_use",
        )?;
        assert!(
            RepositoryStore::at(root.path().join("repositories"))
                .get(&id)?
                .is_some()
        );
        Ok(())
    }

    fn update_request(id: &RepositoryId, path: &Path) -> UpdateRepositoryPathRequest {
        UpdateRepositoryPathRequest {
            repository_id: id.as_str().to_owned(),
            path: path.display().to_string(),
        }
    }

    fn assert_failure_code<T>(
        result: Result<T, RepositoryCommandFailure>,
        expected: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match result {
            Err(failure) => {
                assert_eq!(failure.code, expected);
                Ok(())
            }
            Ok(_) => Err(format!("expected failure code {expected}").into()),
        }
    }
}
