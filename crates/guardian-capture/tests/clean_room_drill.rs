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
//! The managed replacement case additionally proves preservation of the
//! previous tree and automatic rollback after a simulated service restart
//! failure. New-destination deploy still has no rollback mode by design.

#[path = "clean_room_drill/cancellation.rs"]
mod cancellation;
#[path = "clean_room_drill/capture_cancellation.rs"]
mod capture_cancellation;
#[path = "clean_room_drill/deploy_second_payload_failure.rs"]
mod deploy_second_payload_failure;
#[path = "clean_room_drill/disk_exhaustion.rs"]
mod disk_exhaustion;
#[path = "clean_room_drill/host_key_rejection.rs"]
mod host_key_rejection;
mod support;

use guardian_core::{
    CredentialId, ProfileId, RemoteTargetPath, RepositoryId, RunId, SecretStore, SecretValue,
};
use guardian_deploy::{DeploymentComposition, ReplacementComposition};
use guardian_local_repository::LocalRepository;
use guardian_ssh::{PinnedHost, SshUser, SystemOpenSsh};
use std::time::{Duration, Instant};
use support::{Check, Phase, TestResult};

const READY_DEADLINE: Duration = Duration::from_secs(45);
const HOST_KEY_DEADLINE: Duration = Duration::from_secs(30);

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

    let repository_id = RepositoryId::parse("drill-restore-repo")?;
    let repository =
        LocalRepository::open(workdir.path().join("repository"), repository_id.clone())?;
    let (recovery, signer) = support::initialize_and_export(
        workdir.path(),
        &repository,
        repository_id,
        &vault_dir,
        &vault,
    )?;

    let capture = support::capture_drill_backup(
        &repository,
        &ssh,
        &profile,
        &vault,
        &signer,
        "drill-restore-backup",
        "drill-restore-run",
    )?;
    let second_payload_failure = support::create_second_payload_failure_backup(
        &repository,
        &capture.sealed.backup_id,
        &vault,
        &signer,
        &profile,
    )?;
    let hostile_archive =
        support::create_hostile_archive_backup(&repository, &vault, &signer, &profile)?;
    drop(signer);
    drop(vault);
    std::fs::remove_dir_all(&vault_dir)?;
    std::fs::remove_dir_all(workdir.path().join("original-signing"))?;
    std::fs::remove_dir_all(workdir.path().join("original-repositories"))?;

    let hostile_start = Instant::now();
    support::prove_hostile_restore_failures(
        workdir.path(),
        &recovery,
        &capture.sealed.backup_id,
        &second_payload_failure.backup_id,
        &hostile_archive.backup_id,
    )?;
    let hostile_phase = Phase::new("hostile_fail_closed", hostile_start.elapsed());

    let destination = workdir.path().join("restored");
    let restore_start = Instant::now();
    support::restore_on_clean_machine(
        workdir.path(),
        &recovery,
        &capture.sealed.backup_id,
        &destination,
    )?;
    let restore_phase = Phase::new("restore", restore_start.elapsed());

    let verify_start = Instant::now();
    let expected_config = workdir.path().join("expected-config.yaml");
    source.copy_out("/srv/app/config.yaml", &expected_config)?;

    let filesystem_matches =
        std::fs::read(destination.join("srv/app/config.yaml"))? == std::fs::read(&expected_config)?;
    // A SQLite `.backup` is a logical copy through the database engine, not
    // a raw byte copy — its header's own change-counter fields legitimately
    // differ from the source (confirmed by direct inspection: only those
    // two well-known 4-byte header fields ever differ). Verifying via real
    // SQL is both more correct and mirrors `deploy_drill`'s own remote
    // verification, rather than assuming byte-for-byte equality.
    let database = rusqlite::Connection::open(destination.join("database.sqlite"))?;
    let integrity: String = database.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    let seeded_row: String =
        database.query_row("SELECT body FROM notes WHERE id = 1", [], |row| row.get(0))?;
    let database_matches = integrity == "ok" && seeded_row == "clean-room drill seed row";
    let verify_phase = Phase::new("verify", verify_start.elapsed());
    let rto_seconds = restore_start.elapsed().as_secs_f64();

    assert!(
        filesystem_matches,
        "restored filesystem payload did not match the seeded content"
    );
    assert!(
        database_matches,
        "restored database failed integrity check or is missing its seeded row: integrity={integrity:?} row={seeded_row:?}"
    );

    support::write_report(
        "restore",
        capture.sealed.backup_id.as_str(),
        &[
            Phase::new("capture", capture.duration),
            hostile_phase,
            restore_phase,
            verify_phase,
        ],
        &[
            Check::new("filesystem_byte_exact", filesystem_matches),
            Check::new("database_integrity_and_content", database_matches),
            Check::new("compiled_cli_clean_machine_recovery", true),
            Check::new("wrong_passphrase_no_registration", true),
            Check::new("missing_recovery_key_no_partial_target", true),
            Check::new("corrupted_payload_no_partial_target", true),
            Check::new("second_payload_failure_no_partial_target", true),
            Check::new("hostile_archive_metadata_no_partial_target", true),
        ],
        rto_seconds,
    )?;

    Ok(())
}

#[test]
#[ignore = "requires Docker and a real SSH round trip; run via `npm run test:integration:drill`"]
fn filesystem_only_restore_drill() -> TestResult {
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
    let credential = CredentialId::parse("filesystem-only-credential")?;
    vault.store(&credential, &SecretValue::new(std::fs::read(&private_key)?))?;
    let profile = support::drill_profile(
        ProfileId::parse("filesystem-only-source")?,
        credential,
        source.port(),
        &host_key,
    )?;
    let repository = LocalRepository::open(
        workdir.path().join("repository"),
        RepositoryId::parse("filesystem-only-repo")?,
    )?;
    repository.configure_recovery_key(&vault)?;
    let signer = support::TestSigner::new();
    let capture = support::capture_filesystem_only_drill_backup(
        &repository,
        &ssh,
        &profile,
        &vault,
        &signer,
        "filesystem-only-backup",
        "filesystem-only-run",
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
    let verify_start = Instant::now();
    let expected = workdir.path().join("expected.yaml");
    source.copy_out("/srv/app/config.yaml", &expected)?;
    let matches =
        std::fs::read(destination.join("srv/app/config.yaml"))? == std::fs::read(expected)?;
    assert!(
        matches,
        "filesystem-only restore did not reproduce captured content"
    );
    let verify_phase = Phase::new("verify", verify_start.elapsed());
    let rto_seconds = restore_start.elapsed().as_secs_f64();
    support::write_report(
        "filesystem-only-restore",
        capture.sealed.backup_id.as_str(),
        &[
            Phase::new("capture", capture.duration),
            restore_phase,
            verify_phase,
        ],
        &[
            Check::new("filesystem_byte_exact", matches),
            Check::new("no_database_payload", true),
        ],
        rto_seconds,
    )?;
    Ok(())
}

struct NoDockerInventory;
impl guardian_core::DockerInventoryPort for NoDockerInventory {
    fn inspect(
        &self,
        _: &guardian_core::VdsProfile,
    ) -> Result<guardian_core::DockerInventory, guardian_core::DockerInventoryPortError> {
        Ok(guardian_core::DockerInventory {
            containers: Vec::new(),
        })
    }
}

struct FixtureDockerInventory;
impl guardian_core::DockerInventoryPort for FixtureDockerInventory {
    fn inspect(
        &self,
        _: &guardian_core::VdsProfile,
    ) -> Result<guardian_core::DockerInventory, guardian_core::DockerInventoryPortError> {
        Ok(guardian_core::DockerInventory {
            containers: vec![guardian_core::DockerContainer {
                id: "a".repeat(64),
                name: "fixture".to_owned(),
                image: "fixture:1".to_owned(),
                image_digest: None,
                compose_project: None,
                state: guardian_core::DockerContainerState::Running,
                health: Some(guardian_core::DockerHealth::Healthy),
                mounts: vec![guardian_core::DockerMount {
                    kind: guardian_core::DockerMountKind::Bind,
                    source_reference: "/srv/app".to_owned(),
                    host_path: None,
                    destination: "/data".to_owned(),
                    read_only: false,
                }],
                networks: Vec::new(),
                secret_references: Vec::new(),
            }],
        })
    }
}

#[test]
#[ignore = "requires Docker and a real SSH round trip; run via `npm run test:integration:drill`"]
fn replacement_cutover_and_rollback_preservation_drill() -> TestResult {
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
    let credential = CredentialId::parse("replacement-drill-credential")?;
    vault.store(&credential, &SecretValue::new(std::fs::read(&private_key)?))?;
    let profile_id = ProfileId::parse("replacement-drill-source")?;
    let profile = support::drill_profile(profile_id.clone(), credential, source.port(), &host_key)?;
    let repository = LocalRepository::open(
        workdir.path().join("repository"),
        RepositoryId::parse("replacement-drill-repo")?,
    )?;
    repository.configure_recovery_key(&vault)?;
    let signer = support::TestSigner::new();
    let original = support::capture_filesystem_only_drill_backup(
        &repository,
        &ssh,
        &profile,
        &vault,
        &signer,
        "replacement-original",
        "replacement-original-run",
    )?;
    let known_hosts = support::write_known_hosts(workdir.path(), &host)?;
    support::run_verification_command(
        source.port(),
        &private_key,
        &known_hosts,
        "rm -- /srv/app/config.yaml && printf '%s\\n' 'mode: mutated' 'service: vds-guardian-drill' > /srv/app/config.yaml",
    )?;
    let safety = support::capture_filesystem_only_drill_backup(
        &repository,
        &ssh,
        &profile,
        &vault,
        &signer,
        "replacement-safety",
        "replacement-safety-run",
    )?;
    let composition = ReplacementComposition {
        repository: &repository,
        ssh: &ssh,
        target_profile: &profile,
        credentials: &vault,
        verifier: &signer,
        docker_inventory: &NoDockerInventory,
    };
    let plan = composition.plan(&original.sealed.backup_id)?;
    let run_id = RunId::parse("replacement-cutover")?;
    composition.execute(
        &run_id,
        &profile_id,
        &original.sealed.backup_id,
        &safety.sealed.backup_id,
        &plan.impact.confirmation,
    )?;
    let restored = support::run_verification_command(
        source.port(),
        &private_key,
        &known_hosts,
        "cat /srv/app/config.yaml",
    )?;
    let rollback = support::run_verification_command(
        source.port(),
        &private_key,
        &known_hosts,
        "cat /srv/.guardian-rollback.replacement-cutover/config.yaml",
    )?;
    let restored_ok = restored == "mode: drill-fixture\nservice: vds-guardian-drill";
    let rollback_ok = rollback == "mode: mutated\nservice: vds-guardian-drill";
    assert!(
        restored_ok,
        "replacement did not publish the selected sealed backup"
    );
    assert!(
        rollback_ok,
        "replacement did not preserve the immediately previous tree"
    );
    support::run_verification_command(
        source.port(),
        &private_key,
        &known_hosts,
        "rm -- /srv/app/config.yaml && printf '%s\\n' 'mode: rollback-payload' 'service: vds-guardian-drill' > /srv/app/config.yaml",
    )?;
    let rollback_payload = support::capture_replacement_workload_drill_backup(
        &repository,
        &ssh,
        &profile,
        &vault,
        &signer,
        "replacement-rollback-payload",
        "replacement-rollback-payload-run",
    )?;
    support::run_verification_command(
        source.port(),
        &private_key,
        &known_hosts,
        "rm -- /srv/app/config.yaml && printf '%s\\n' 'mode: live-before-failure' 'service: vds-guardian-drill' > /srv/app/config.yaml",
    )?;
    let rollback_safety = support::capture_filesystem_only_drill_backup(
        &repository,
        &ssh,
        &profile,
        &vault,
        &signer,
        "replacement-rollback-safety",
        "replacement-rollback-safety-run",
    )?;
    source.install_fail_once_docker(workdir.path())?;
    let rollback_composition = ReplacementComposition {
        repository: &repository,
        ssh: &ssh,
        target_profile: &profile,
        credentials: &vault,
        verifier: &signer,
        docker_inventory: &FixtureDockerInventory,
    };
    let rollback_plan = rollback_composition.plan(&rollback_payload.sealed.backup_id)?;
    let rollback_run = RunId::parse("replacement-auto-rollback")?;
    let failure = rollback_composition.execute(
        &rollback_run,
        &profile_id,
        &rollback_payload.sealed.backup_id,
        &rollback_safety.sealed.backup_id,
        &rollback_plan.impact.confirmation,
    );
    assert!(matches!(
        failure,
        Err(guardian_deploy::ReplacementError::RolledBack)
    ));
    let live_after_failure = support::run_verification_command(
        source.port(),
        &private_key,
        &known_hosts,
        "cat /srv/app/config.yaml",
    )?;
    let automatic_rollback_ok =
        live_after_failure == "mode: live-before-failure\nservice: vds-guardian-drill";
    assert!(
        automatic_rollback_ok,
        "failed service restart did not restore the live tree"
    );
    let rolled_back_audit = workdir
        .path()
        .join("repository/audit/replacement-replacement-auto-rollback-rolled_back.json")
        .is_file();
    assert!(rolled_back_audit, "rolled-back cutover was not audited");
    support::write_report(
        "replacement",
        original.sealed.backup_id.as_str(),
        &[
            Phase::new("capture", original.duration),
            Phase::new("safety_backup", safety.duration),
        ],
        &[
            Check::new("replacement_content", restored_ok),
            Check::new("rollback_tree_preserved", rollback_ok),
            Check::new(
                "automatic_rollback_after_restart_failure",
                automatic_rollback_ok,
            ),
            Check::new("rolled_back_audit", rolled_back_audit),
        ],
        original.duration.as_secs_f64() + safety.duration.as_secs_f64(),
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
    repository.configure_recovery_key(&vault)?;
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

    let verify_start = Instant::now();
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
    let verify_phase = Phase::new("verify", verify_start.elapsed());
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
