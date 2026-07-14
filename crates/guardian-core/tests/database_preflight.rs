use guardian_core::{
    CredentialId, DatabaseCapability, DatabaseCapabilityProbeError, DatabaseCapabilityProbePort,
    DatabaseEngine, DatabasePreflightError, DatabasePreflightUseCase, DatabaseVersion, HostPin,
    ProfileId, ProfileStorePort, ProfileStorePortError, SshEndpoint, VdsProfile,
};

#[test]
fn database_preflight_rejects_major_version_mismatch() -> Result<(), Box<dyn std::error::Error>> {
    let profile = profile()?;
    let store = Store(profile.clone());
    let probe = Probe;
    let result = DatabasePreflightUseCase {
        profiles: &store,
        probe: &probe,
    }
    .execute(&profile.profile_id);
    assert_eq!(result, Err(DatabasePreflightError::IncompatibleVersion));
    Ok(())
}

#[test]
fn database_preflight_rejects_missing_capabilities() -> Result<(), Box<dyn std::error::Error>> {
    let profile = profile()?;
    let profiles = Store(profile.clone());
    let probe = EmptyProbe;
    let result = DatabasePreflightUseCase {
        profiles: &profiles,
        probe: &probe,
    }
    .execute(&profile.profile_id);
    assert_eq!(result, Err(DatabasePreflightError::NoCapabilities));
    Ok(())
}

struct EmptyProbe;

impl DatabaseCapabilityProbePort for EmptyProbe {
    fn probe(
        &self,
        _: &VdsProfile,
    ) -> Result<Vec<DatabaseCapability>, DatabaseCapabilityProbeError> {
        Ok(Vec::new())
    }
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

struct Store(VdsProfile);

impl ProfileStorePort for Store {
    fn save(&self, _: VdsProfile) -> Result<(), ProfileStorePortError> {
        Ok(())
    }

    fn get(&self, id: &ProfileId) -> Result<Option<VdsProfile>, ProfileStorePortError> {
        Ok((self.0.profile_id == *id).then(|| self.0.clone()))
    }
}

struct Probe;

impl DatabaseCapabilityProbePort for Probe {
    fn probe(
        &self,
        _: &VdsProfile,
    ) -> Result<Vec<DatabaseCapability>, DatabaseCapabilityProbeError> {
        Ok(vec![DatabaseCapability {
            engine: DatabaseEngine::PostgreSql,
            server_version: DatabaseVersion {
                major: 16,
                minor: 4,
                patch: 0,
            },
            dump_tool_version: DatabaseVersion {
                major: 15,
                minor: 8,
                patch: 0,
            },
        }])
    }
}
