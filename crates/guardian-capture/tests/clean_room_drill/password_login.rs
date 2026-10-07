//! Password authentication through the real stack: the compiled `guardian-askpass`
//! helper, the one-shot loopback broker, system OpenSSH and a real `sshd` that
//! allows `root` to log in with a password. Nothing is mocked between the
//! credential vault and the wire.

use super::{HOST_KEY_DEADLINE, READY_DEADLINE, support};
use guardian_archive::ArchiveLimits;
use guardian_capture::{FilesystemCaptureComposition, SYSTEM_DISK_SPACE};
use guardian_core::{
    CredentialId, FilesystemBackupRequest, FilesystemCaptureRequest, PayloadPath, ProfileId,
    RepositoryId, RunId, SecretStore, SecretValue, Timestamp,
};
use guardian_local_repository::LocalRepository;
use guardian_ssh::{PinnedHost, SshIdentity, SshUser, SystemOpenSsh};
use std::{error::Error, path::Path, path::PathBuf, time::Duration};

struct Outcome {
    sealed: bool,
    repository_root: PathBuf,
    backup_id: String,
    _workdir: tempfile::TempDir,
}

/// Registers the program OpenSSH runs as `SSH_ASKPASS`: the stand-alone helper built beside the
/// test binaries, or the executable named by `GUARDIAN_DRILL_ASKPASS` (used to prove that the
/// real, windowed desktop executable works as the helper).
fn register_helper() -> Result<(), Box<dyn Error>> {
    static REGISTERED: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    REGISTERED
        .get_or_init(|| {
            let program = std::env::var_os("GUARDIAN_DRILL_ASKPASS")
                .map(PathBuf::from)
                .or_else(|| {
                    let executable = std::env::current_exe().ok()?;
                    let name = format!("guardian-askpass{}", std::env::consts::EXE_SUFFIX);
                    Some(executable.parent()?.parent()?.join(name))
                })
                .ok_or("cannot locate the askpass helper")?;
            guardian_ssh::register_password_helper(program).map_err(|error| error.to_string())
        })
        .clone()
        .map_err(Into::into)
}

/// Starts the fixture and waits until its sshd answers, using the key route only for readiness.
fn start_ready() -> Result<(support::Container, String), Box<dyn Error>> {
    register_helper()?;
    let image = support::fixture_image()?;
    let container = support::Container::start(image)?;
    let workdir = tempfile::tempdir()?;
    let (private_key, public_key) = support::generate_keypair(workdir.path())?;
    container.install_public_key(&public_key)?;
    let host_key = container.host_key_base64(HOST_KEY_DEADLINE)?;
    let ssh = SystemOpenSsh::default();
    let ready = PinnedHost::parse(
        "127.0.0.1",
        container.port(),
        "ssh-ed25519",
        host_key.clone(),
    )?;
    support::wait_until_ssh_ready(
        &ssh,
        &ready,
        &SshUser::parse("backup")?,
        &private_key,
        READY_DEADLINE,
    )?;
    Ok((container, host_key))
}

/// One complete production capture as `root`, authenticating with `password` and pinning `pinned_host_key`.
fn capture_as_root(
    name: &str,
    container: &support::Container,
    pinned_host_key: &str,
    password: &str,
) -> Result<Outcome, Box<dyn Error>> {
    let workdir = tempfile::tempdir()?;
    let vault_dir = workdir.path().join("vault");
    std::fs::create_dir(&vault_dir)?;
    let vault = support::open_vault(&vault_dir)?;
    let credential = CredentialId::parse(format!("drill-{name}-credential"))?;
    vault.store(
        &credential,
        &SecretValue::new(SshIdentity::encode_password(password)?),
    )?;
    let profile = support::drill_profile_as(
        "root",
        ProfileId::parse(format!("drill-{name}-source"))?,
        credential,
        container.port(),
        pinned_host_key,
    )?;
    let repository = LocalRepository::open(
        workdir.path().join("repository"),
        RepositoryId::parse(format!("drill-{name}-repository"))?,
    )?;
    repository.configure_recovery_key(&vault)?;
    let audit = support::NoopAudit;
    let capture = FilesystemCaptureComposition {
        repository: &repository,
        ssh: &SystemOpenSsh::default(),
        profile: &profile,
        credentials: &vault,
        audit: &audit,
        disk_space: &SYSTEM_DISK_SPACE,
        archive_limits: ArchiveLimits::conservative(),
    };
    let run_id = RunId::parse(format!("drill-{name}"))?;
    let backup_id = format!("drill-{name}-backup");
    let request = FilesystemBackupRequest {
        capture: FilesystemCaptureRequest {
            run_id: run_id.clone(),
            profile_id: profile.profile_id.clone(),
            roots: vec!["/srv/app".to_owned()],
            payload_path: PayloadPath::parse("payload/filesystem-000.tar.zst.enc")?,
        },
        manifest: support::drill_manifest(&backup_id, run_id, &profile)?,
        sealed_at: Timestamp::parse("2026-10-07T12:00:01Z")?,
    };
    let sealed = capture
        .execute(request, None, &support::TestSigner::new())
        .is_ok();
    Ok(Outcome {
        sealed,
        repository_root: repository.root().to_path_buf(),
        backup_id,
        _workdir: workdir,
    })
}

fn contains_bytes(directory: &Path, needle: &[u8]) -> Result<bool, Box<dyn Error>> {
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            if contains_bytes(&path, needle)? {
                return Ok(true);
            }
        } else if std::fs::read(&path)?
            .windows(needle.len())
            .any(|window| window == needle)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn count_in_logs(
    container: &support::Container,
    needle: &str,
    expected: usize,
) -> Result<usize, Box<dyn Error>> {
    let mut count = 0;
    for _ in 0..15 {
        count = container.logs()?.matches(needle).count();
        if count >= expected {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(count)
}

#[test]
#[ignore = "requires Docker and a real SSH round trip; run via `npm run test:integration:drill`"]
fn a_password_login_seals_an_encrypted_backup_and_leaks_the_password_nowhere()
-> Result<(), Box<dyn Error>> {
    let (container, host_key) = start_ready()?;
    let outcome = capture_as_root(
        "password-ok",
        &container,
        &host_key,
        support::FIXTURE_ROOT_PASSWORD,
    )?;
    assert!(outcome.sealed, "capture over a password login failed");
    assert!(
        outcome
            .repository_root
            .join("backups")
            .join(&outcome.backup_id)
            .is_dir()
    );
    assert!(
        !contains_bytes(
            &outcome.repository_root,
            support::FIXTURE_ROOT_PASSWORD.as_bytes()
        )?,
        "the password must not appear anywhere in the repository"
    );
    assert!(count_in_logs(&container, "Accepted password for root", 1)? >= 1);
    Ok(())
}

#[test]
#[ignore = "requires Docker and a real SSH round trip; run via `npm run test:integration:drill`"]
fn a_wrong_password_is_tried_exactly_once_and_seals_nothing() -> Result<(), Box<dyn Error>> {
    let (container, host_key) = start_ready()?;
    let outcome = capture_as_root(
        "password-wrong",
        &container,
        &host_key,
        "definitely-not-the-password",
    )?;
    assert!(!outcome.sealed, "capture succeeded with a wrong password");
    assert!(
        !outcome
            .repository_root
            .join("backups")
            .join(&outcome.backup_id)
            .exists()
    );
    assert_eq!(
        count_in_logs(&container, "Failed password for root", 1)?,
        1,
        "a wrong password must be attempted once, never retried"
    );
    Ok(())
}

#[test]
#[ignore = "requires Docker and a real SSH round trip; run via `npm run test:integration:drill`"]
fn the_password_is_never_offered_to_a_host_that_does_not_match_the_pin()
-> Result<(), Box<dyn Error>> {
    let (container, _real_host_key) = start_ready()?;
    let impostor = support::Container::start(support::fixture_image()?)?;
    let wrong_pin = impostor.host_key_base64(HOST_KEY_DEADLINE)?;
    let outcome = capture_as_root(
        "password-pin",
        &container,
        &wrong_pin,
        support::FIXTURE_ROOT_PASSWORD,
    )?;
    assert!(
        !outcome.sealed,
        "capture accepted a host that does not match the pinned key"
    );
    std::thread::sleep(Duration::from_millis(500));
    let logs = container.logs()?;
    assert!(
        !logs.contains("Accepted password") && !logs.contains("Failed password"),
        "no password authentication may reach a host whose key does not match the pin:\n{logs}"
    );
    Ok(())
}
