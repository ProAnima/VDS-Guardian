//! The desktop's "fetch host key" lookup against a real `sshd`, with `ssh-keygen` as an
//! independent oracle for the fingerprint the operator is asked to compare.

use super::{HOST_KEY_DEADLINE, support};
use guardian_ssh::{openssh_fingerprint, scan_host_key};
use std::{
    error::Error,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires Docker and a real SSH round trip; run via `npm run test:integration:drill`"]
fn the_scanned_key_and_fingerprint_are_the_servers_own() -> Result<(), Box<dyn Error>> {
    let container = support::Container::start(support::fixture_image()?)?;
    let expected = container.host_key_base64(HOST_KEY_DEADLINE)?;
    let scanned = scan_host_key("127.0.0.1", container.port(), Duration::from_secs(10))?;
    assert_eq!(scanned.algorithm, "ssh-ed25519");
    assert_eq!(scanned.public_key_base64, expected);

    let workdir = tempfile::tempdir()?;
    let public = workdir.path().join("host.pub");
    std::fs::write(&public, format!("ssh-ed25519 {expected}\n"))?;
    let oracle = std::process::Command::new("ssh-keygen")
        .arg("-lf")
        .arg(&public)
        .output()?;
    let printed = String::from_utf8_lossy(&oracle.stdout);
    assert!(
        printed.contains(&scanned.fingerprint),
        "ssh-keygen printed {printed}, we showed {}",
        scanned.fingerprint
    );
    assert_eq!(scanned.fingerprint, openssh_fingerprint(&expected)?);
    Ok(())
}

#[test]
#[ignore = "requires Docker and a real SSH round trip; run via `npm run test:integration:drill`"]
fn a_closed_port_fails_quickly_instead_of_hanging() -> Result<(), Box<dyn Error>> {
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let started = Instant::now();
    assert!(scan_host_key("127.0.0.1", port, Duration::from_secs(5)).is_err());
    assert!(started.elapsed() < Duration::from_secs(15));
    Ok(())
}
