mod deploy_commands;
mod docker_commands;
mod job_commands;
mod plan_commands;
mod profile_commands;
mod profile_delete;
mod remote_browser_commands;
mod repository_commands;
mod restore_commands;
mod signing_commands;

use guardian_core::{FoundationStatus, JobRegistry};
use guardian_signing::{SigningIdentityEnrollment, SigningIdentityFailure, SigningIdentityStatus};
use tauri::Manager;

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
async fn delete_ssh_profile(
    app: tauri::AppHandle,
    request: profile_delete::DeleteProfileRequest,
) -> Result<(), profile_delete::DeleteProfileFailure> {
    profile_delete::delete(app, request).await
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
async fn initialize_repository_recovery(
    app: tauri::AppHandle,
    repository_id: String,
) -> Result<(), repository_commands::RepositoryCommandFailure> {
    repository_commands::initialize_recovery(app, repository_id).await
}

#[tauri::command]
async fn export_recovery_bundle(
    app: tauri::AppHandle,
    request: repository_commands::ExportRecoveryBundleRequest,
) -> Result<(), repository_commands::RepositoryCommandFailure> {
    repository_commands::export_recovery_bundle_file(app, request).await
}

#[tauri::command]
async fn import_recovery_bundle(
    app: tauri::AppHandle,
    request: repository_commands::ImportRecoveryBundleRequest,
) -> Result<repository_commands::RepositorySummary, repository_commands::RepositoryCommandFailure> {
    repository_commands::import_recovery_bundle_file(app, request).await
}

#[tauri::command]
async fn save_capture_plan(
    app: tauri::AppHandle,
    request: plan_commands::SavePlanRequest,
) -> Result<plan_commands::PlanSummary, plan_commands::PlanFailure> {
    plan_commands::save(app, request).await
}

#[tauri::command]
async fn preview_capture_selection(
    app: tauri::AppHandle,
    request: guardian_core::BackupSelection,
) -> Result<guardian_core::CaptureSelectionPreview, plan_commands::PlanFailure> {
    plan_commands::preview(app, request).await
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

#[tauri::command]
async fn run_capture_selection(
    app: tauri::AppHandle,
    request: job_commands::RunCaptureSelectionRequest,
) -> Result<job_commands::CaptureJobSummary, job_commands::CaptureJobFailure> {
    job_commands::run_selection(app, request).await
}

#[tauri::command]
async fn list_backups(
    app: tauri::AppHandle,
    repository_id: String,
) -> Result<Vec<restore_commands::BackupSummary>, restore_commands::RestoreFailure> {
    restore_commands::list(app, repository_id).await
}

#[tauri::command]
async fn preview_restore(
    app: tauri::AppHandle,
    request: restore_commands::RestoreRequest,
) -> Result<restore_commands::RestorePreview, restore_commands::RestoreFailure> {
    restore_commands::preview(app, request).await
}

#[tauri::command]
async fn execute_restore(
    app: tauri::AppHandle,
    request: restore_commands::RestoreRequest,
) -> Result<restore_commands::RestorePreview, restore_commands::RestoreFailure> {
    restore_commands::execute(app, request).await
}

#[tauri::command]
async fn list_docker_containers(
    app: tauri::AppHandle,
    profile_id: String,
) -> Result<Vec<docker_commands::DockerContainerSummary>, docker_commands::DockerCommandFailure> {
    docker_commands::list_containers(app, profile_id).await
}

#[tauri::command]
async fn browse_remote_directory(
    app: tauri::AppHandle,
    request: remote_browser_commands::BrowseDirectoryRequest,
) -> Result<guardian_core::RemoteBrowsePage, remote_browser_commands::BrowseDirectoryFailure> {
    remote_browser_commands::browse(app, request).await
}

#[tauri::command]
async fn preview_deploy(
    app: tauri::AppHandle,
    request: deploy_commands::DeployRequest,
) -> Result<deploy_commands::DeploymentPreview, deploy_commands::DeployFailure> {
    deploy_commands::preview(app, request).await
}

#[tauri::command]
async fn execute_deploy(
    app: tauri::AppHandle,
    request: deploy_commands::DeployRequest,
) -> Result<deploy_commands::DeploymentPreview, deploy_commands::DeployFailure> {
    deploy_commands::execute(app, request).await
}

/// Signals cancellation for a still-running capture, restore, or deploy job, if one is
/// registered under this run id. Synchronous and near-instant (a lock plus a
/// flag store) — no `spawn_blocking` needed, unlike the long-running jobs it
/// cancels. Returns whether a matching job was found, not whether it has
/// actually stopped yet — cancellation is cooperative, checked on the job's
/// own next poll tick.
#[tauri::command]
fn cancel_job(app: tauri::AppHandle, run_id: String) -> bool {
    let Ok(run_id) = guardian_core::RunId::parse(run_id) else {
        return false;
    };
    app.state::<JobRegistry>().cancel(&run_id)
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(JobRegistry::default())
        .invoke_handler(tauri::generate_handler![
            get_foundation_status,
            get_signing_identity_status,
            enroll_signing_identity,
            enroll_ssh_profile,
            list_ssh_profiles,
            delete_ssh_profile,
            test_ssh_profile,
            preflight_ssh_profile,
            register_repository,
            list_repositories,
            initialize_repository_recovery,
            export_recovery_bundle,
            import_recovery_bundle,
            save_capture_plan,
            preview_capture_selection,
            list_capture_plans,
            run_capture_plan,
            run_capture_selection,
            list_backups,
            preview_restore,
            execute_restore,
            preview_deploy,
            execute_deploy,
            cancel_job,
            list_docker_containers,
            browse_remote_directory
        ])
        .run(tauri::generate_context!())?;
    Ok(())
}
