//! Automated clean-room restore/deploy drill. Calls real, already-shipped
//! composition roots (capture, local restore, remote deploy) against
//! disposable Docker fixtures over a real SSH round trip — closing the
//! "no automated clean-room restore drill exists" gap named in
//! `docs/DEVELOPMENT_PLAN.md` (Milestones 1, 3, 4) and
//! `docs/OPERATIONS_RUNBOOK.md`'s "Restore drill" section.
//!
//! Requires Docker and real SSH/`ssh-keygen` binaries; both tests are
//! `#[ignore]`d so `cargo test --workspace --all-targets` (part of
//! `npm run verify`, which runs on both Windows and Linux CI) stays fast
//! and Docker-free. Run explicitly via `npm run test:integration:drill`,
//! wired into CI on the Linux leg only, after the existing SSH gate.
//!
//! Does not prove restore/deploy rollback — that feature does not exist
//! yet. Each report records `rollback.proven: false` rather than silently
//! omitting or overclaiming that clause.

mod support;

use guardian_core::{
    CredentialId, ProfileId, RemoteTargetPath, RepositoryId, RunId, SecretStore, SecretValue,
};
use guardian_deploy::DeploymentComposition;
use guardian_local_repository::LocalRepository;
use guardian_ssh::{PinnedHost, SshUser, SystemOpenSsh};
use std::time::{Duration, Instant};
use support::{Check, Phase, TestResult};

const READY_DEADLINE: Duration = Duration::from_secs(15);
const HOST_KEY_DEADLINE: Duration = Duration::from_secs(10);

#[test]
#[ignore = "requires Docker and a real SSH round trip; run via `npm run test:integration:drill`"]
fn restore_drill() -> TestResult {
    let image = support::fixture_image()?;
    let source = support::Container::start(image)?;
    let workdir = tempfile::tempdir()?;
    let (private_key, public_key) = support::generate_keypair(workdir.path())?;
    source.install_public_key(&public_key)?;
    let host_key = source.host_key_base64(HOST_KEY_DEADLINE)?;

    let ssh = SystemOpenSsh::default();
    let host = PinnedHost::parse("127.0.0.1", source.port(), "ssh-ed25519", host_key.clone())?;
    let user = SshUser::parse("backup")?;
    support::wait_until_ssh_ready(&ssh, &host, &user, &private_key, READY_DEADLINE)?;

    let vault_dir = workdir.path().join("vault");
    std::fs::create_dir(&vault_dir)?;
    let vault = support::open_vault(&vault_dir)?;
    let credential_id = CredentialId::parse("drill-restore-credential")?;
    vault.store(
        &credential_id,
        &SecretValue::new(std::fs::read(&private_key)?),
    )?;

    let profile_id = ProfileId::parse("drill-restore-source")?;
    let profile = support::drill_profile(profile_id, credential_id, source.port(), &host_key)?;

    let repository = LocalRepository::open(
        workdir.path().join("repository"),
        RepositoryId::parse("drill-restore-repo")?,
    )?;
    let signer = support::TestSigner::new();

    let capture = support::capture_drill_backup(
        &repository,
        &ssh,
        &profile,
        &vault,
        &signer,
        "drill-restore-backup",
        "drill-restore-run",
    )?;

    let destination = workdir.path().join("restored");
    let restore_start = Instant::now();
    let plan = repository.plan_restore(&capture.sealed.backup_id, &destination, &signer)?;
    repository.execute_restore(
        &capture.sealed.backup_id,
        &destination,
        &plan.confirmation,
        &signer,
        &vault,
    )?;
    let restore_phase = Phase::new("restore", restore_start.elapsed());

    let expected_config = workdir.path().join("expected-config.yaml");
    source.copy_out("/srv/app/config.yaml", &expected_config)?;
    let expected_database = workdir.path().join("expected-app.sqlite");
    source.copy_out("/srv/app/app.sqlite", &expected_database)?;

    let filesystem_matches =
        std::fs::read(destination.join("srv/app/config.yaml"))? == std::fs::read(&expected_config)?;
    let restored_database = std::fs::read(destination.join("database.sqlite"))?;
    let database_matches = restored_database == std::fs::read(&expected_database)?
        && restored_database.starts_with(b"SQLite format 3\0");
    let verify_phase = Phase::new("verify", restore_start.elapsed());
    let rto_seconds = restore_start.elapsed().as_secs_f64();

    assert!(
        filesystem_matches,
        "restored filesystem payload did not match the seeded content"
    );
    assert!(
        database_matches,
        "restored database payload did not byte-match the seeded database"
    );

    support::write_report(
        "restore",
        capture.sealed.backup_id.as_str(),
        &[
            Phase::new("capture", capture.duration),
            restore_phase,
            verify_phase,
        ],
        &[
            Check::new("filesystem_byte_exact", filesystem_matches),
            Check::new("database_byte_exact", database_matches),
        ],
        rto_seconds,
    )?;

    Ok(())
}

#[test]
#[ignore = "requires Docker and a real SSH round trip; run via `npm run test:integration:drill`"]
fn deploy_drill() -> TestResult {
    let image = support::fixture_image()?;
    let source = support::Container::start(image)?;
    let target = support::Container::start(image)?;
    let workdir = tempfile::tempdir()?;
    let (private_key, public_key) = support::generate_keypair(workdir.path())?;
    source.install_public_key(&public_key)?;
    target.install_public_key(&public_key)?;
    let source_host_key = source.host_key_base64(HOST_KEY_DEADLINE)?;
    let target_host_key = target.host_key_base64(HOST_KEY_DEADLINE)?;

    let ssh = SystemOpenSsh::default();
    let user = SshUser::parse("backup")?;
    let source_host = PinnedHost::parse(
        "127.0.0.1",
        source.port(),
        "ssh-ed25519",
        source_host_key.clone(),
    )?;
    let target_host = PinnedHost::parse(
        "127.0.0.1",
        target.port(),
        "ssh-ed25519",
        target_host_key.clone(),
    )?;
    support::wait_until_ssh_ready(&ssh, &source_host, &user, &private_key, READY_DEADLINE)?;
    support::wait_until_ssh_ready(&ssh, &target_host, &user, &private_key, READY_DEADLINE)?;

    let vault_dir = workdir.path().join("vault");
    std::fs::create_dir(&vault_dir)?;
    let vault = support::open_vault(&vault_dir)?;
    let source_credential = CredentialId::parse("drill-deploy-source-credential")?;
    let target_credential = CredentialId::parse("drill-deploy-target-credential")?;
    let private_key_bytes = std::fs::read(&private_key)?;
    vault.store(
        &source_credential,
        &SecretValue::new(private_key_bytes.clone()),
    )?;
    vault.store(&target_credential, &SecretValue::new(private_key_bytes))?;

    let source_profile_id = ProfileId::parse("drill-deploy-source")?;
    let source_profile = support::drill_profile(
        source_profile_id,
        source_credential,
        source.port(),
        &source_host_key,
    )?;
    let target_profile_id = ProfileId::parse("drill-deploy-target")?;
    let target_profile = support::drill_profile(
        target_profile_id.clone(),
        target_credential,
        target.port(),
        &target_host_key,
    )?;

    let repository = LocalRepository::open(
        workdir.path().join("repository"),
        RepositoryId::parse("drill-deploy-repo")?,
    )?;
    let signer = support::TestSigner::new();

    let capture = support::capture_drill_backup(
        &repository,
        &ssh,
        &source_profile,
        &vault,
        &signer,
        "drill-deploy-backup",
        "drill-deploy-run",
    )?;

    let deployment = DeploymentComposition {
        repository: &repository,
        ssh: &ssh,
        target_profile: &target_profile,
        credentials: &vault,
        verifier: &signer,
    };
    let target_path = RemoteTargetPath::parse("/srv/drill-deploy")?;
    let deploy_start = Instant::now();
    let plan = deployment.plan(&capture.sealed.backup_id, target_path.clone())?;
    deployment.execute(
        &RunId::parse("drill-deploy-push")?,
        &target_profile_id,
        &capture.sealed.backup_id,
        target_path,
        &plan.confirmation,
    )?;
    let deploy_phase = Phase::new("deploy", deploy_start.elapsed());

    let known_hosts = support::write_known_hosts(workdir.path(), &target_host)?;
    let integrity = support::run_verification_command(
        target.port(),
        &private_key,
        &known_hosts,
        "sqlite3 /srv/drill-deploy/database.sqlite \"PRAGMA integrity_check;\"",
    )?;
    let seeded_row = support::run_verification_command(
        target.port(),
        &private_key,
        &known_hosts,
        "sqlite3 /srv/drill-deploy/database.sqlite \"SELECT body FROM notes WHERE id = 1;\"",
    )?;
    let deployed_config = support::run_verification_command(
        target.port(),
        &private_key,
        &known_hosts,
        "cat /srv/drill-deploy/srv/app/config.yaml",
    )?;
    let verify_phase = Phase::new("verify", deploy_start.elapsed());
    let rto_seconds = deploy_start.elapsed().as_secs_f64();

    let database_integrity_ok = integrity == "ok";
    let database_data_ok = seeded_row == "clean-room drill seed row";
    let filesystem_ok = deployed_config == "mode: drill-fixture\nservice: vds-guardian-drill";

    assert!(
        database_integrity_ok,
        "deployed database failed PRAGMA integrity_check: {integrity}"
    );
    assert!(
        database_data_ok,
        "deployed database did not contain the seeded row: {seeded_row:?}"
    );
    assert!(
        filesystem_ok,
        "deployed filesystem payload did not match the seeded content: {deployed_config:?}"
    );

    support::write_report(
        "deploy",
        capture.sealed.backup_id.as_str(),
        &[
            Phase::new("capture", capture.duration),
            deploy_phase,
            verify_phase,
        ],
        &[
            Check::new("database_integrity_check", database_integrity_ok),
            Check::new("database_seeded_row", database_data_ok),
            Check::new("filesystem_content", filesystem_ok),
        ],
        rto_seconds,
    )?;

    Ok(())
}
