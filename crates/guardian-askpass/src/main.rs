//! `SSH_ASKPASS` helper: prints the login password for OpenSSH, or nothing and a failing
//! exit status. It never writes a secret anywhere except the pipe OpenSSH reads from.

use guardian_askpass::{PORT_VARIABLE, TOKEN_VARIABLE, request_password};
use std::{env, io::Write, process::ExitCode};

fn main() -> ExitCode {
    let prompt = env::args().nth(1).unwrap_or_default();
    let (Ok(port), Ok(token)) = (env::var(PORT_VARIABLE), env::var(TOKEN_VARIABLE)) else {
        return refuse();
    };
    let Ok(password) = request_password(&prompt, &port, &token) else {
        return refuse();
    };
    let mut line = password.to_vec();
    line.push(b'\n');
    let written = std::io::stdout()
        .lock()
        .write_all(&line)
        .and_then(|()| std::io::stdout().flush());
    zeroize_line(&mut line);
    if written.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn zeroize_line(line: &mut [u8]) {
    line.fill(0);
}

fn refuse() -> ExitCode {
    eprintln!("guardian-askpass: password request refused");
    ExitCode::FAILURE
}
