#![allow(dead_code)]

#[path = "support/recovery.rs"]
mod recovery;

pub use recovery::{
    initialize_and_export, prove_hostile_restore_failures, restore_on_clean_machine,
};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier};
use guardian_archive::ArchiveLimits;
use guardian_capture::FilesystemCaptureComposition;
use guardian_core::{
    AuditPort, BackupId, CaptureAuditCode, CredentialId, EmbeddedDatabaseCaptureRequest,
    FilesystemBackupRequest, FilesystemCaptureRequest, HostPin, Manifest, ManifestSigner,
    PayloadPath, PlanId, PlanReference, Producer, ProfileId, RunId, SealedBackup, SecretStore,
    SigningError, SourceIdentity, SshEndpoint, Timestamp, VdsProfile,
};
use guardian_local_repository::LocalRepository;
use guardian_ssh::{PinnedHost, SshUser, SystemOpenSsh};
use guardian_vault::EncryptedFileVault;
use serde::Serialize;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub type TestResult = Result<(), Box<dyn Error>>;

const IMAGE: &str = "vds-guardian-drill-fixture:local";
const FIXTURE_USER: &str = "backup";
static IMAGE_BUILT: OnceLock<Result<(), String>> = OnceLock::new();

/// Builds the drill fixture image once per test binary invocation, even if
/// both `#[test]` functions in this file happen to run concurrently, so
/// two tests never race a duplicate `docker build`.
pub fn fixture_image() -> Result<&'static str, Box<dyn Error>> {
    let outcome = IMAGE_BUILT.get_or_init(|| {
        let fixture_dir = fixture_directory();
        run(
            "docker",
            &[
                "build",
                "--pull=false",
                "-t",
                IMAGE,
                &fixture_dir.to_string_lossy(),
            ],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
    });
    match outcome {
        Ok(()) => Ok(IMAGE),
        Err(message) => Err(message.clone().into()),
    }
}

fn fixture_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("drill-fixture")
}

/// A disposable fixture container, removed with `docker rm -f` on drop —
/// mirroring `scripts/test-ssh-integration.mjs`'s existing try/finally
/// cleanup. A failed removal does not fail the test.
pub struct Container {
    id: String,
    port: u16,
}

impl Container {
    /// Starts a fresh container from `image`, overriding `CMD` at run time
    /// (rather than baking `ssh-keygen -A` into the image, as the existing
    /// SSH fixture does) so every container generates its own host key the
    /// first time it boots. Two containers from this same image must never
    /// share a host-key fingerprint — the deploy drill's self-overwrite
    /// guard compares fingerprints, and a shared baked-in key would make
    /// two genuinely different hosts look identical to it.
    pub fn start(image: &str) -> Result<Self, Box<dyn Error>> {
        let id = run(
            "docker",
            &[
                "run",
                "-d",
                "-p",
                "127.0.0.1::22",
                image,
                "sh",
                "-c",
                "ssh-keygen -A && exec /usr/sbin/sshd -D -e",
            ],
        )?;
        match discover_port(&id) {
            Ok(port) => Ok(Self { id, port }),
            Err(error) => {
                let _ = run("docker", &["rm", "-f", &id]);
                Err(error)
            }
        }
    }

    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn install_public_key(&self, public_key_path: &Path) -> Result<(), Box<dyn Error>> {
        run(
            "docker",
            &[
                "cp",
                &public_key_path.to_string_lossy(),
                &format!("{}:/home/{FIXTURE_USER}/.ssh/authorized_keys", self.id),
            ],
        )?;
        run(
            "docker",
            &[
                "exec",
                &self.id,
                "chown",
                &format!("{FIXTURE_USER}:{FIXTURE_USER}"),
                &format!("/home/{FIXTURE_USER}/.ssh/authorized_keys"),
            ],
        )?;
        Ok(())
    }

    /// Polls for the host key this container generates at boot, since
    /// (unlike the existing SSH fixture) it is never baked into the image.
    pub fn host_key_base64(&self, deadline: Duration) -> Result<String, Box<dyn Error>> {
        let start = Instant::now();
        loop {
            if let Ok(line) = run(
                "docker",
                &["exec", &self.id, "cat", "/etc/ssh/ssh_host_ed25519_key.pub"],
            ) && let Some(key) = line.split_ascii_whitespace().nth(1)
            {
                return Ok(key.to_owned());
            }
            if start.elapsed() >= deadline {
                return Err("timed out waiting for the fixture's host key".into());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    pub fn copy_out(&self, remote_path: &str, local_path: &Path) -> Result<(), Box<dyn Error>> {
        run(
            "docker",
            &[
                "cp",
                &format!("{}:{remote_path}", self.id),
                &local_path.to_string_lossy(),
            ],
        )?;
        Ok(())
    }
}

impl Drop for Container {
    fn drop(&mut self) {
        let _ = run("docker", &["rm", "-f", &self.id]);
    }
}

fn discover_port(container_id: &str) -> Result<u16, Box<dyn Error>> {
    let endpoint = run("docker", &["port", container_id, "22/tcp"])?;
    endpoint
        .rsplit(':')
        .next()
        .and_then(|value| value.trim().parse::<u16>().ok())
        .ok_or_else(|| format!("unable to discover fixture SSH port from {endpoint:?}").into())
}

/// Generates a fresh, unencrypted ed25519 keypair by shelling out to real
/// `ssh-keygen` rather than hand-encoding `ed25519-dalek` key bytes into
/// OpenSSH's own private-key wire format — the existing SSH fixture script
/// already does this on the same Linux-only CI leg this drill runs on.
pub fn generate_keypair(directory: &Path) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let private_key = directory.join("id_ed25519");
    run(
        "ssh-keygen",
        &[
            "-q",
            "-t",
            "ed25519",
            "-N",
            "",
            "-f",
            &private_key.to_string_lossy(),
        ],
    )?;
    let public_key = directory.join("id_ed25519.pub");
    Ok((private_key, public_key))
}

/// Waits until the pinned adapter can actually complete an SSH round trip,
/// not just until the container is "running". This container generates its
/// host key at boot rather than at image-build time, adding a second
/// readiness dimension beyond what the existing fixture script's flat
/// sleep needed to cover.
pub fn wait_until_ssh_ready(
    ssh: &SystemOpenSsh,
    host: &PinnedHost,
    user: &SshUser,
    identity_path: &Path,
    deadline: Duration,
) -> Result<(), Box<dyn Error>> {
    let start = Instant::now();
    loop {
        if ssh.probe_connection(host, user, identity_path).is_ok() {
            return Ok(());
        }
        if start.elapsed() >= deadline {
            let mut known_hosts = tempfile::NamedTempFile::new()?;
            use std::io::Write as _;
            known_hosts.write_all(host.known_hosts_line().as_bytes())?;
            known_hosts.flush()?;
            let known_hosts = known_hosts.into_temp_path();
            let output = std::process::Command::new("ssh")
                .args(ssh.connection_probe_arguments(
                    host,
                    user,
                    identity_path,
                    known_hosts.as_ref(),
                ))
                .output()?;
            return Err(format!(
                "timed out waiting for fixture SSH; known_hosts={:?}; final probe: {}",
                host.known_hosts_line(),
                String::from_utf8_lossy(&output.stderr).trim()
            )
            .into());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

pub struct TestSigner {
    key: SigningKey,
}

impl TestSigner {
    pub fn new() -> Self {
        Self {
            key: SigningKey::from_bytes(&[7_u8; 32]),
        }
    }
}

impl Default for TestSigner {
    fn default() -> Self {
        Self::new()
    }
}

impl ManifestSigner for TestSigner {
    fn algorithm(&self) -> &'static str {
        "Ed25519"
    }

    fn key_id(&self) -> &str {
        "drill-ed25519-key"
    }

    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, SigningError> {
        Ok(self.key.sign(message).to_bytes().to_vec())
    }

    fn verify(&self, message: &[u8], signature: &[u8]) -> Result<(), SigningError> {
        let signature =
            Signature::from_slice(signature).map_err(|_| SigningError::VerificationFailed)?;
        self.key
            .verifying_key()
            .verify(message, &signature)
            .map_err(|_| SigningError::VerificationFailed)
    }
}

pub struct NoopAudit;

impl AuditPort for NoopAudit {
    fn capture_failed(&self, _: &RunId, _: CaptureAuditCode) {}
}

/// Opens (initializing first if needed) a real, working `SecretStore`
/// backed by `guardian-vault`'s already-shipped encrypted file vault. Used
/// instead of an in-memory fake because capture and restore/deploy must
/// round-trip the *same* payload data key through a real store, not merely
/// tolerate one that always trivially succeeds.
pub fn open_vault(directory: &Path) -> Result<EncryptedFileVault, Box<dyn Error>> {
    EncryptedFileVault::init(directory)?;
    Ok(EncryptedFileVault::open(directory)?)
}

pub fn drill_profile(
    profile_id: ProfileId,
    credential_id: CredentialId,
    port: u16,
    host_key_base64: &str,
) -> Result<VdsProfile, Box<dyn Error>> {
    Ok(VdsProfile {
        profile_id,
        label: "Clean-room drill fixture".to_owned(),
        credential_id,
        endpoint: SshEndpoint {
            host: "127.0.0.1".to_owned(),
            port,
            user: FIXTURE_USER.to_owned(),
            host_pin: HostPin::parse("ssh-ed25519", host_key_base64)?,
        },
    })
}

/// Builds a fresh manifest for a drill capture, computing the source
/// host-key fingerprint from the *real* pinned profile rather than a
/// placeholder string — the deploy drill's self-overwrite guard compares
/// this fingerprint against the target profile's, so it has to be real for
/// that check to mean anything.
pub fn drill_manifest(
    backup_id: &str,
    run_id: RunId,
    profile: &VdsProfile,
) -> Result<Manifest, Box<dyn Error>> {
    Ok(Manifest::new(
        BackupId::parse(backup_id)?,
        run_id,
        Timestamp::parse("2026-07-15T12:00:00Z")?,
        Producer {
            name: "vds-guardian-clean-room-drill".to_owned(),
            version: "0.1.0".to_owned(),
            platform: "test".to_owned(),
        },
        SourceIdentity {
            profile_id: profile.profile_id.clone(),
            host_key_fingerprint: guardian_core::host_key_fingerprint(
                &profile.endpoint.host_pin.public_key_base64,
            ),
        },
        PlanReference {
            plan_id: PlanId::parse("clean-room-drill-plan")?,
            version: 1,
            sha256: "a".repeat(64),
        },
    ))
}

pub struct CaptureOutcome {
    pub sealed: SealedBackup,
    pub duration: Duration,
}

/// Runs one real combined filesystem+database capture against `profile`
/// through the actual `guardian-capture` composition root — shared by both
/// drills so `clean_room_drill.rs` states each test's setup once rather
/// than repeating this identical sequence twice.
pub fn capture_drill_backup(
    repository: &LocalRepository,
    ssh: &SystemOpenSsh,
    profile: &VdsProfile,
    credentials: &dyn SecretStore,
    signer: &dyn ManifestSigner,
    backup_id: &str,
    run_id: &str,
) -> Result<CaptureOutcome, Box<dyn Error>> {
    let audit = NoopAudit;
    let composition = FilesystemCaptureComposition {
        repository,
        ssh,
        profile,
        credentials,
        audit: &audit,
        archive_limits: ArchiveLimits::conservative(),
    };
    let run_id = RunId::parse(run_id)?;
    let capture = FilesystemCaptureRequest {
        run_id: run_id.clone(),
        profile_id: profile.profile_id.clone(),
        roots: vec!["/srv/app".to_owned()],
        payload_path: PayloadPath::parse("payload/filesystem-000.tar.zst.enc")?,
    };
    let database = EmbeddedDatabaseCaptureRequest {
        run_id: run_id.clone(),
        profile_id: profile.profile_id.clone(),
        database_path: "/srv/app/app.sqlite".to_owned(),
        payload_path: PayloadPath::parse("payload/database-000.sqlite.zst.enc")?,
    };
    let manifest = drill_manifest(backup_id, run_id, profile)?;
    let request = FilesystemBackupRequest {
        capture,
        manifest,
        sealed_at: Timestamp::parse("2026-07-15T12:00:01Z")?,
    };
    let start = Instant::now();
    let sealed = composition.execute(request, Some(database), signer)?;
    Ok(CaptureOutcome {
        sealed,
        duration: start.elapsed(),
    })
}

pub fn write_known_hosts(directory: &Path, host: &PinnedHost) -> Result<PathBuf, Box<dyn Error>> {
    let path = directory.join("known_hosts");
    std::fs::write(&path, host.known_hosts_line())?;
    Ok(path)
}

/// A raw, test-only SSH invocation for verifying a *result* the drill
/// itself produced (e.g. querying a deployed database with real SQL) —
/// never a new production capability. Uses the same strict flags as
/// `SystemOpenSsh`'s own pinned adapter and as
/// `scripts/test-ssh-integration.mjs`'s existing hand-rolled verification
/// calls, so even this harness-only call stays pinned and batch-mode.
pub fn run_verification_command(
    port: u16,
    identity_path: &Path,
    known_hosts_path: &Path,
    remote_command: &str,
) -> Result<String, Box<dyn Error>> {
    let output = Command::new("ssh")
        .args([
            "-F",
            "none",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "StrictHostKeyChecking=yes",
        ])
        .arg("-o")
        .arg(format!("UserKnownHostsFile={}", known_hosts_path.display()))
        .args([
            "-o",
            "GlobalKnownHostsFile=none",
            "-o",
            "PasswordAuthentication=no",
            "-o",
            "KbdInteractiveAuthentication=no",
            "-o",
            "PreferredAuthentications=publickey",
            "-o",
            "IdentitiesOnly=yes",
        ])
        .arg("-i")
        .arg(identity_path)
        .arg("-p")
        .arg(port.to_string())
        .arg(format!("{FIXTURE_USER}@127.0.0.1"))
        .arg(remote_command)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "verification ssh command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[derive(Serialize)]
pub struct Phase {
    pub name: &'static str,
    pub duration_ms: u128,
    pub status: &'static str,
}

impl Phase {
    pub fn new(name: &'static str, duration: Duration) -> Self {
        Self {
            name,
            duration_ms: duration.as_millis(),
            status: "pass",
        }
    }
}

#[derive(Serialize)]
pub struct Check {
    pub name: &'static str,
    pub status: &'static str,
}

impl Check {
    pub fn new(name: &'static str, passed: bool) -> Self {
        Self {
            name,
            status: if passed { "pass" } else { "fail" },
        }
    }
}

#[derive(Serialize)]
struct RollbackNote {
    proven: bool,
    note: &'static str,
}

#[derive(Serialize)]
struct DrillReport<'a> {
    drill: &'a str,
    backup_id: &'a str,
    phases: &'a [Phase],
    checks: &'a [Check],
    rto_seconds: f64,
    rto_scope: &'static str,
    rpo: Option<()>,
    rpo_note: &'static str,
    rollback: RollbackNote,
}

/// Writes the machine-readable report `OPERATIONS_RUNBOOK.md`'s "Restore
/// drill" section requires. RTO is scoped to restore/deploy-plus-verify,
/// deliberately excluding capture/seal (a real disaster restores an
/// already-captured backup, so timing capture into "recovery time" would
/// overstate it); RPO is left absent with an explanation rather than
/// fabricated, since a single drill run cannot measure a scheduling-policy
/// concern.
pub fn write_report(
    drill: &str,
    backup_id: &str,
    phases: &[Phase],
    checks: &[Check],
    rto_seconds: f64,
) -> Result<(), Box<dyn Error>> {
    let report = DrillReport {
        drill,
        backup_id,
        phases,
        checks,
        rto_seconds,
        rto_scope: "restore_start_to_verify_complete",
        rpo: None,
        rpo_note: "RPO is a scheduling-policy concern (backup frequency); a single drill run cannot measure it. Scheduling is unbuilt M5 work.",
        rollback: RollbackNote {
            proven: false,
            note: "restore/deploy rollback is not implemented yet; out of scope for this drill.",
        },
    };
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("drill-reports");
    std::fs::create_dir_all(&directory)?;
    let path = directory.join(format!("{drill}-report.json"));
    std::fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
    println!("clean-room drill report written to {}", path.display());
    Ok(())
}

fn run(program: &str, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        return Err(format!(
            "{program} {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
