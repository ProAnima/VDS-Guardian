use super::{HOST_KEY_DEADLINE, READY_DEADLINE, support};
use guardian_archive::ArchiveLimits;
use guardian_capture::FilesystemCaptureComposition;
use guardian_core::{
    CancellationHandle, CredentialId, FilesystemBackupRequest, FilesystemCaptureRequest,
    JobRegistry, PayloadPath, ProfileId, RepositoryId, RunId, SecretStore, SecretValue, Timestamp,
};
use guardian_local_repository::LocalRepository;
use guardian_ssh::{PinnedHost, SshUser, SystemOpenSsh};
use std::{error::Error, thread, time::Duration};

#[test]
#[ignore = "requires Docker and a real SSH round trip; run via `npm run test:integration:drill`"]
fn capture_cancellation_drill() -> Result<(), Box<dyn Error>> {
    let image = support::fixture_image()?;
    let source = support::Container::start(image)?;
    let workdir = tempfile::tempdir()?;
    let (private_key, public_key) = support::generate_keypair(workdir.path())?;
    let stream_marker = source.install_throttled_capture_key(&public_key, workdir.path())?;
    source.add_incompressible_fixture_file()?;
    let host_key = source.host_key_base64(HOST_KEY_DEADLINE)?;
    let ready_ssh = SystemOpenSsh::default();
    let user = SshUser::parse("backup")?;
    let host = PinnedHost::parse("127.0.0.1", source.port(), "ssh-ed25519", host_key.clone())?;
    support::wait_until_ssh_ready(&ready_ssh, &host, &user, &private_key, READY_DEADLINE)?;

    let vault_dir = workdir.path().join("vault");
    std::fs::create_dir(&vault_dir)?;
    let vault = support::open_vault(&vault_dir)?;
    let credential = CredentialId::parse("drill-capture-cancel-credential")?;
    vault.store(&credential, &SecretValue::new(std::fs::read(&private_key)?))?;
    let profile = support::drill_profile(
        ProfileId::parse("drill-capture-cancel-source")?,
        credential,
        source.port(),
        &host_key,
    )?;
    let repository = LocalRepository::open(
        workdir.path().join("repository"),
        RepositoryId::parse("drill-capture-cancel-repository")?,
    )?;
    repository.configure_recovery_key(&vault)?;
    let handle = CancellationHandle::new();
    let capture_ssh = SystemOpenSsh::default().with_cancellation(handle.clone());
    let audit = support::NoopAudit;
    let capture = FilesystemCaptureComposition {
        repository: &repository,
        ssh: &capture_ssh,
        profile: &profile,
        credentials: &vault,
        audit: &audit,
        archive_limits: ArchiveLimits::conservative(),
    };
    let run_id = RunId::parse("drill-capture-cancel")?;
    let backup_id = "drill-capture-cancel-backup";
    let request = FilesystemBackupRequest {
        capture: FilesystemCaptureRequest {
            run_id: run_id.clone(),
            profile_id: profile.profile_id.clone(),
            roots: vec!["/srv/app".to_owned()],
            payload_path: PayloadPath::parse("payload/filesystem-000.tar.zst.enc")?,
        },
        manifest: support::drill_manifest(backup_id, run_id.clone(), &profile)?,
        sealed_at: Timestamp::parse("2026-07-16T12:00:01Z")?,
    };
    let signer = support::TestSigner::new();
    let jobs = JobRegistry::default();
    let registration = jobs.register(run_id.clone(), handle);

    thread::scope(|scope| -> Result<(), Box<dyn Error>> {
        let running_capture = scope.spawn(|| capture.execute(request, None, &signer));
        source.wait_for_remote_file(stream_marker, Duration::from_secs(15))?;
        if !jobs.cancel(&run_id) {
            return Err("operator cancellation did not find the running capture".into());
        }
        let result = running_capture
            .join()
            .map_err(|_| "cancelled capture thread panicked")?;
        if result.is_ok() {
            return Err("capture succeeded after operator cancellation".into());
        }
        Ok(())
    })?;
    drop(registration);

    let root = repository.root();
    assert!(!root.join("staging").join(run_id.as_str()).exists());
    assert!(!root.join("backups").join(backup_id).exists());
    let audit = root.join("audit");
    assert!(
        audit
            .join("capture-drill-capture-cancel-started.json")
            .is_file()
    );
    assert!(
        audit
            .join("capture-drill-capture-cancel-cancelled.json")
            .is_file()
    );
    assert!(
        !audit
            .join("capture-drill-capture-cancel-sealed.json")
            .exists()
    );
    assert!(
        !audit
            .join("capture-drill-capture-cancel-failed.json")
            .exists()
    );
    assert!(!jobs.cancel(&run_id));
    Ok(())
}
