use guardian_configuration::{CapturePlanStore, RepositoryStore, StoredCapturePlan};
use guardian_core::{FilesystemCapturePlan, PlanId, ProfileId, ProfileStorePort, RepositoryId};
use guardian_profile_store::ProfileStore;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Manager;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavePlanRequest {
    profile_id: String,
    repository_id: String,
    roots: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlanSummary {
    pub plan_id: String,
    pub profile_id: String,
    pub repository_id: String,
    pub roots: Vec<String>,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanFailure {
    pub code: &'static str,
    pub message: &'static str,
    pub remediation: &'static str,
}

pub async fn save(
    app: tauri::AppHandle,
    request: SavePlanRequest,
) -> Result<PlanSummary, PlanFailure> {
    let root = app
        .path()
        .app_config_dir()
        .map_err(|_| PlanFailure::storage())?;
    tauri::async_runtime::spawn_blocking(move || save_blocking(root, request))
        .await
        .map_err(|_| PlanFailure::internal())?
}

pub async fn list(app: tauri::AppHandle) -> Result<Vec<PlanSummary>, PlanFailure> {
    let root = app
        .path()
        .app_config_dir()
        .map_err(|_| PlanFailure::storage())?;
    tauri::async_runtime::spawn_blocking(move || list_blocking(root))
        .await
        .map_err(|_| PlanFailure::internal())?
}

fn save_blocking(root: PathBuf, request: SavePlanRequest) -> Result<PlanSummary, PlanFailure> {
    let profile_id = ProfileId::parse(request.profile_id).map_err(|_| PlanFailure::invalid())?;
    let repository_id =
        RepositoryId::parse(request.repository_id).map_err(|_| PlanFailure::invalid())?;
    ProfileStore::at(root.join("profiles"))
        .get(&profile_id)
        .map_err(|_| PlanFailure::storage())?
        .ok_or_else(PlanFailure::invalid_reference)?;
    RepositoryStore::at(root.join("repositories"))
        .get(&repository_id)
        .map_err(|_| PlanFailure::storage())?
        .ok_or_else(PlanFailure::invalid_reference)?;
    let plan = FilesystemCapturePlan {
        plan_id: PlanId::parse(random_id()).map_err(|_| PlanFailure::internal())?,
        version: 1,
        profile_id,
        repository_id,
        roots: request.roots,
    };
    let stored = StoredCapturePlan::new(plan).map_err(|_| PlanFailure::invalid())?;
    CapturePlanStore::at(root.join("plans"))
        .upsert(stored.clone())
        .map_err(|_| PlanFailure::storage())?;
    Ok(PlanSummary::from(&stored))
}

fn list_blocking(root: PathBuf) -> Result<Vec<PlanSummary>, PlanFailure> {
    let profiles = ProfileStore::at(root.join("profiles"));
    let repositories = RepositoryStore::at(root.join("repositories"));
    let plans = CapturePlanStore::at(root.join("plans"))
        .list()
        .map_err(|_| PlanFailure::storage())?;
    for stored in &plans {
        profiles
            .get(&stored.plan.profile_id)
            .map_err(|_| PlanFailure::storage())?
            .ok_or_else(PlanFailure::invalid_reference)?;
        repositories
            .get(&stored.plan.repository_id)
            .map_err(|_| PlanFailure::storage())?
            .ok_or_else(PlanFailure::invalid_reference)?;
    }
    Ok(plans.iter().map(PlanSummary::from).collect())
}

fn random_id() -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    format!(
        "plan-{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

impl From<&StoredCapturePlan> for PlanSummary {
    fn from(value: &StoredCapturePlan) -> Self {
        Self {
            plan_id: value.plan.plan_id.as_str().to_owned(),
            profile_id: value.plan.profile_id.as_str().to_owned(),
            repository_id: value.plan.repository_id.as_str().to_owned(),
            roots: value.plan.roots.clone(),
            sha256: value.sha256.clone(),
        }
    }
}

impl PlanFailure {
    fn invalid() -> Self {
        Self {
            code: "invalid_capture_plan",
            message: "The capture plan is invalid.",
            remediation: "Choose one server, one repository, and one or more absolute server paths without traversal.",
        }
    }
    fn invalid_reference() -> Self {
        Self {
            code: "missing_capture_plan_reference",
            message: "The selected server or repository does not exist.",
            remediation: "Refresh setup data and select an existing server and backup location.",
        }
    }
    fn storage() -> Self {
        Self {
            code: "plan_storage_unavailable",
            message: "The capture plan could not be saved.",
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
