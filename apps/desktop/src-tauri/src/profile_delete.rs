use guardian_configuration::CapturePlanStore;
use guardian_core::ProfileId;
use guardian_os_keyring::OsCredentialStore;
use guardian_profile_store::{ProfileDeletionError, ProfileStore};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Manager;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteProfileRequest {
    profile_id: String,
    confirmed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteProfileFailure {
    pub code: &'static str,
    pub message: &'static str,
    pub remediation: &'static str,
}

pub async fn delete(
    app: tauri::AppHandle,
    request: DeleteProfileRequest,
) -> Result<(), DeleteProfileFailure> {
    let root = app
        .path()
        .app_config_dir()
        .map_err(|_| DeleteProfileFailure::storage())?;
    tauri::async_runtime::spawn_blocking(move || delete_blocking(root, request))
        .await
        .map_err(|_| DeleteProfileFailure::internal())?
}

fn delete_blocking(
    root: PathBuf,
    request: DeleteProfileRequest,
) -> Result<(), DeleteProfileFailure> {
    if !request.confirmed {
        return Err(DeleteProfileFailure::confirmation());
    }
    let profile_id =
        ProfileId::parse(request.profile_id).map_err(|_| DeleteProfileFailure::not_found())?;
    ensure_profile_is_unused(&root, &profile_id)?;
    ProfileStore::at(root.join("profiles"))
        .remove_with_secret(&profile_id, &OsCredentialStore)
        .map_err(map_deletion_error)?
        .then_some(())
        .ok_or_else(DeleteProfileFailure::not_found)
}

fn ensure_profile_is_unused(
    root: &std::path::Path,
    profile_id: &ProfileId,
) -> Result<(), DeleteProfileFailure> {
    let plans = CapturePlanStore::at(root.join("plans"))
        .list()
        .map_err(|_| DeleteProfileFailure::storage())?;
    if plans
        .iter()
        .any(|stored| stored.plan.profile_id == *profile_id)
    {
        return Err(DeleteProfileFailure::in_use());
    }
    Ok(())
}

fn map_deletion_error(error: ProfileDeletionError) -> DeleteProfileFailure {
    match error {
        ProfileDeletionError::Store(_) => DeleteProfileFailure::storage(),
        ProfileDeletionError::Secret(_) | ProfileDeletionError::Rollback => {
            DeleteProfileFailure::credential_store()
        }
    }
}

impl DeleteProfileFailure {
    fn confirmation() -> Self {
        Self {
            code: "profile_deletion_not_confirmed",
            message: "Server deletion was not confirmed.",
            remediation: "Review the selected server and confirm deletion explicitly.",
        }
    }
    fn in_use() -> Self {
        Self {
            code: "profile_in_use",
            message: "This server is used by a saved backup plan.",
            remediation: "Remove or replace the server in its backup plan before deleting it.",
        }
    }
    fn not_found() -> Self {
        Self {
            code: "profile_not_found",
            message: "The server profile was not found.",
            remediation: "Refresh the server list; it may already have been deleted.",
        }
    }
    fn credential_store() -> Self {
        Self {
            code: "credential_cleanup_failed",
            message: "The server key could not be removed from secure storage.",
            remediation: "Unlock the operating-system credential store and try again; the server profile was kept.",
        }
    }
    fn storage() -> Self {
        Self {
            code: "profile_storage_unavailable",
            message: "The server profile could not be deleted.",
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

#[cfg(test)]
mod tests {
    use super::ensure_profile_is_unused;
    use guardian_configuration::{CapturePlanStore, StoredCapturePlan};
    use guardian_core::{FilesystemCapturePlan, PlanId, ProfileId, RepositoryId};

    #[test]
    fn saved_plan_blocks_profile_deletion() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let profile_id = ProfileId::parse("profile-001")?;
        CapturePlanStore::at(root.path().join("plans")).upsert(StoredCapturePlan::new(
            FilesystemCapturePlan {
                plan_id: PlanId::parse("plan-001")?,
                version: 1,
                profile_id: profile_id.clone(),
                repository_id: RepositoryId::parse("repository-001")?,
                roots: vec!["/srv/app".to_owned()],
                database_path: None,
            },
        )?)?;

        let Err(failure) = ensure_profile_is_unused(root.path(), &profile_id) else {
            return Err("referenced profile was not rejected".into());
        };
        assert_eq!(failure.code, "profile_in_use");
        Ok(())
    }

    #[test]
    fn unused_profile_can_be_deleted() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let profile_id = ProfileId::parse("profile-001")?;
        assert!(ensure_profile_is_unused(root.path(), &profile_id).is_ok());
        Ok(())
    }
}
