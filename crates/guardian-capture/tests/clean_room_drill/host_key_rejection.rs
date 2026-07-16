use super::{HOST_KEY_DEADLINE, READY_DEADLINE, support};
use guardian_archive::ArchiveLimits;
use guardian_capture::FilesystemCaptureComposition;
use guardian_core::{
    CredentialId, FilesystemBackupRequest, FilesystemCaptureRequest, PayloadPath, ProfileId,
    RepositoryId, RunId, SecretStore, SecretValue, Timestamp,
};
use guardian_local_repository::LocalRepository;
use guardian_ssh::{PinnedHost, SshUser, SystemOpenSsh};
use std::error::Error;

#[test]
#[ignore = "requires Docker and a real SSH round trip; run via `npm run test:integration:drill`"]
fn changed_host_key_rejects_capture_before_staging() -> Result<(), Box<dyn Error>> {
    let image = support::fixture_image()?;
    let source = support::Container::start(image)?;
    let different_host = support::Container::start(image)?;
    let workdir = tempfile::tempdir()?;
    let (private_key, public_key) = support::generate_keypair(workdir.path())?;
    source.install_public_key(&public_key)?;
    let source_host_key = source.host_key_base64(HOST_KEY_DEADLINE)?;
    let changed_host_key = different_host.host_key_base64(HOST_KEY_DEADLINE)?;
    let ssh = SystemOpenSsh::default();
    let user = SshUser::parse("backup")?;
    let working_host =
        PinnedHost::parse("127.0.0.1", source.port(), "ssh-ed25519", source_host_key)?;
    support::wait_until_ssh_ready(&ssh, &working_host, &user, &private_key, READY_DEADLINE)?;

    let vault_dir = workdir.path().join("vault");
    std::fs::create_dir(&vault_dir)?;
    let vault = support::open_vault(&vault_dir)?;
    let credential = CredentialId::parse("drill-changed-host-key-credential")?;
    vault.store(&credential, &SecretValue::new(std::fs::read(&private_key)?))?;
    let profile = support::drill_profile(
        ProfileId::parse("drill-changed-host-key-source")?,
        credential,
        source.port(),
        &changed_host_key,
    )?;
    let repository = LocalRepository::open(
        workdir.path().join("repository"),
        RepositoryId::parse("drill-changed-host-key-repository")?,
    )?;
    repository.configure_recovery_key(&vault)?;
    let audit = support::NoopAudit;
    let capture = FilesystemCaptureComposition {
        repository: &repository,
        ssh: &ssh,
        profile: &profile,
        credentials: &vault,
        audit: &audit,
        archive_limits: ArchiveLimits::conservative(),
    };
    let run_id = RunId::parse("drill-changed-host-key")?;
    let backup_id = "drill-changed-host-key-backup";
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
    if capture
        .execute(request, None, &support::TestSigner::new())
        .is_ok()
    {
        return Err("capture accepted a changed SSH host key".into());
    }

    let root = repository.root();
    assert!(!root.join("staging").join(run_id.as_str()).exists());
    assert!(!root.join("backups").join(backup_id).exists());
    let audit = root.join("audit");
    assert!(
        audit
            .join("capture-drill-changed-host-key-started.json")
            .is_file()
    );
    assert!(
        audit
            .join("capture-drill-changed-host-key-failed.json")
            .is_file()
    );
    assert!(
        !audit
            .join("capture-drill-changed-host-key-sealed.json")
            .exists()
    );
    assert!(
        !audit
            .join("capture-drill-changed-host-key-cancelled.json")
            .exists()
    );
    Ok(())
}
