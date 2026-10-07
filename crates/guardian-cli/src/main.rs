use std::process::ExitCode;

fn main() -> ExitCode {
    // OpenSSH runs this same executable as its SSH_ASKPASS program for password logins.
    if let Some(code) = guardian_ssh::init_password_helper() {
        return u8::try_from(code).map_or(ExitCode::FAILURE, ExitCode::from);
    }
    guardian_cli::run(std::env::args_os().skip(1))
}
