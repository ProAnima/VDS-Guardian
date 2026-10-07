#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // OpenSSH runs this same executable as its SSH_ASKPASS program for password logins.
    if let Some(code) = guardian_ssh::init_password_helper() {
        std::process::exit(code);
    }
    vds_guardian_desktop_lib::run()
}
