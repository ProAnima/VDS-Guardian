mod job_commands;
mod plan_commands;
mod profile_commands;
mod repository_commands;
mod signing_commands;

use guardian_core::FoundationStatus;
use guardian_signing::{SigningIdentityEnrollment, SigningIdentityFailure, SigningIdentityStatus};

#[tauri::command]
fn get_foundation_status() -> FoundationStatus {
    FoundationStatus::current()
}

#[tauri::command]
async fn get_signing_identity_status(
    app: tauri::AppHandle,
) -> Result<SigningIdentityStatus, SigningIdentityFailure> {
    signing_commands::status(app).await
}

#[tauri::command]
async fn enroll_signing_identity(
    app: tauri::AppHandle,
) -> Result<SigningIdentityEnrollment, SigningIdentityFailure> {
    signing_commands::enroll(app).await
}

#[tauri::command]
async fn enroll_ssh_profile(
    app: tauri::AppHandle,
    request: profile_commands::EnrollSshProfileRequest,
) -> Result<profile_commands::ProfileSummary, profile_commands::ProfileCommandFailure> {
    profile_commands::enroll(app, request).await
}

#[tauri::command]
async fn list_ssh_profiles(
    app: tauri::AppHandle,
) -> Result<Vec<profile_commands::ProfileSummary>, profile_commands::ProfileCommandFailure> {
    profile_commands::list(app).await
}

#[tauri::command]
async fn test_ssh_profile(
    app: tauri::AppHandle,
    profile_id: String,
) -> Result<(), profile_commands::ProfileCommandFailure> {
    profile_commands::test(app, profile_id).await
}

#[tauri::command]
async fn preflight_ssh_profile(
    app: tauri::AppHandle,
    profile_id: String,
) -> Result<profile_commands::SshPreflightSummary, profile_commands::ProfileCommandFailure> {
    profile_commands::preflight(app, profile_id).await
}

#[tauri::command]
async fn register_repository(
    app: tauri::AppHandle,
    request: repository_commands::RegisterRepositoryRequest,
) -> Result<repository_commands::RepositorySummary, repository_commands::RepositoryCommandFailure> {
    repository_commands::register(app, request).await
}

#[tauri::command]
async fn list_repositories(
    app: tauri::AppHandle,
) -> Result<
    Vec<repository_commands::RepositorySummary>,
    repository_commands::RepositoryCommandFailure,
> {
    repository_commands::list(app).await
}

#[tauri::command]
async fn save_capture_plan(
    app: tauri::AppHandle,
    request: plan_commands::SavePlanRequest,
) -> Result<plan_commands::PlanSummary, plan_commands::PlanFailure> {
    plan_commands::save(app, request).await
}

#[tauri::command]
async fn list_capture_plans(
    app: tauri::AppHandle,
) -> Result<Vec<plan_commands::PlanSummary>, plan_commands::PlanFailure> {
    plan_commands::list(app).await
}

#[tauri::command]
async fn run_capture_plan(
    app: tauri::AppHandle,
    request: job_commands::RunCapturePlanRequest,
) -> Result<job_commands::CaptureJobSummary, job_commands::CaptureJobFailure> {
    job_commands::run(app, request).await
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_foundation_status,
            get_signing_identity_status,
            enroll_signing_identity,
            enroll_ssh_profile,
            list_ssh_profiles,
            test_ssh_profile,
            preflight_ssh_profile,
            register_repository,
            list_repositories,
            save_capture_plan,
            list_capture_plans,
            run_capture_plan
        ])
        .run(tauri::generate_context!())?;
    Ok(())
}
