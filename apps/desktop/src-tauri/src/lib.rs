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

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_foundation_status,
            get_signing_identity_status,
            enroll_signing_identity
        ])
        .run(tauri::generate_context!())?;
    Ok(())
}
