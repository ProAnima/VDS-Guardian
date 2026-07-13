use guardian_core::FoundationStatus;

#[tauri::command]
fn get_foundation_status() -> FoundationStatus {
    FoundationStatus::current()
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_foundation_status])
        .run(tauri::generate_context!())?;
    Ok(())
}
