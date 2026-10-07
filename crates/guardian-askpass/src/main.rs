//! Stand-alone `SSH_ASKPASS` helper used by tests and tooling. Applications do not ship it:
//! they call [`guardian_askpass::run_if_requested`] at the start of `main` instead, so the
//! program OpenSSH runs is the application's own (signed) executable.

use std::process::ExitCode;

fn main() -> ExitCode {
    match guardian_askpass::run_if_requested() {
        Some(0) => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}
