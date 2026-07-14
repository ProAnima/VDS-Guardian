use std::process::ExitCode;

fn main() -> ExitCode {
    guardian_cli::run(std::env::args_os().skip(1))
}
