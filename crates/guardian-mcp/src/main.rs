fn main() -> Result<(), Box<dyn std::error::Error>> {
    // OpenSSH runs this same executable as its SSH_ASKPASS program for password logins.
    if let Some(code) = guardian_ssh::init_password_helper() {
        std::process::exit(code);
    }
    let arguments: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    guardian_mcp::run(&arguments)
}
