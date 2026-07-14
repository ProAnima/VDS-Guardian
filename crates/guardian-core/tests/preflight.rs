use guardian_core::{
    CredentialId, HostPin, PreflightSshCaptureError, PreflightSshCaptureUseCase, ProfileId,
    ProfileStorePort, ProfileStorePortError, SshCapabilityProbeError, SshCapabilityProbePort,
    SshCaptureCapabilities, SshEndpoint, VdsProfile,
};
use std::sync::Mutex;

#[test]
fn preflight_rejects_missing_capability_before_capture() -> Result<(), Box<dyn std::error::Error>> {
    let profile = profile()?;
    let profiles = Store::with(profile.clone());
    let probe = Probe::new(false);
    let result = PreflightSshCaptureUseCase {
        profiles: &profiles,
        probe: &probe,
    }
    .execute(&profile.profile_id);
    assert_eq!(result, Err(PreflightSshCaptureError::TarZstdUnsupported));
    Ok(())
}

#[test]
fn preflight_never_probes_a_missing_profile() -> Result<(), Box<dyn std::error::Error>> {
    let profiles = Store::default();
    let probe = Probe::new(true);
    let result = PreflightSshCaptureUseCase {
        profiles: &profiles,
        probe: &probe,
    }
    .execute(&ProfileId::parse("missing-profile")?);
    assert_eq!(result, Err(PreflightSshCaptureError::ProfileNotFound));
    assert_eq!(probe.calls.lock().map_err(|_| "lock")?.to_owned(), 0);
    Ok(())
}

fn profile() -> Result<VdsProfile, Box<dyn std::error::Error>> {
    Ok(VdsProfile {
        profile_id: ProfileId::parse("profile-001")?,
        label: "VDS".to_owned(),
        credential_id: CredentialId::parse("credential-001")?,
        endpoint: SshEndpoint {
            host: "vds.example".to_owned(),
            port: 22,
            user: "backup".to_owned(),
            host_pin: HostPin::parse("ssh-ed25519", "AAAAC3NzaC1lZDI1NTE5AQ==")?,
        },
    })
}

#[derive(Default)]
struct Store {
    profile: Option<VdsProfile>,
}

impl Store {
    fn with(profile: VdsProfile) -> Self {
        Self {
            profile: Some(profile),
        }
    }
}

impl ProfileStorePort for Store {
    fn save(&self, _: VdsProfile) -> Result<(), ProfileStorePortError> {
        Ok(())
    }

    fn get(&self, id: &ProfileId) -> Result<Option<VdsProfile>, ProfileStorePortError> {
        Ok(self
            .profile
            .as_ref()
            .filter(|profile| profile.profile_id == *id)
            .cloned())
    }
}

struct Probe {
    tar_zstd: bool,
    calls: Mutex<usize>,
}

impl Probe {
    fn new(tar_zstd: bool) -> Self {
        Self {
            tar_zstd,
            calls: Mutex::new(0),
        }
    }
}

impl SshCapabilityProbePort for Probe {
    fn probe(&self, _: &VdsProfile) -> Result<SshCaptureCapabilities, SshCapabilityProbeError> {
        let mut calls = self
            .calls
            .lock()
            .map_err(|_| SshCapabilityProbeError::Unavailable)?;
        *calls += 1;
        Ok(SshCaptureCapabilities {
            tar_zstd: self.tar_zstd,
        })
    }
}
