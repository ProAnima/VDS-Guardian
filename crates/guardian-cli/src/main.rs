use guardian_core::FoundationStatus;
use std::process::ExitCode;

fn main() -> ExitCode {
    let status = FoundationStatus::current();
    match serde_json::to_string_pretty(&status) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("failed to serialize foundation status: {error}");
            ExitCode::FAILURE
        }
    }
}
