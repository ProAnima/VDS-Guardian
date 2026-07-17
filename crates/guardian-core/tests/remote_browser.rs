use guardian_core::{
    BrowseRemoteDirectoryError, BrowseRemoteDirectoryUseCase, CredentialId, HostPin, ProfileId,
    ProfileStorePort, ProfileStorePortError, RemoteBrowseEntry, RemoteBrowsePage,
    RemoteBrowseRequest, RemoteBrowserPort, RemoteBrowserPortError, RemoteEntryKind,
    RemoteEntryUnavailableReason, RemotePath, SshEndpoint, VdsProfile,
};

#[test]
fn valid_bounded_directory_page_is_accepted() -> Result<(), Box<dyn std::error::Error>> {
    let profile = profile()?;
    let request = request(20)?;
    let browser = Browser { page: page(false)? };
    let store = Store(profile.clone());
    let result = BrowseRemoteDirectoryUseCase {
        profiles: &store,
        browser: &browser,
    }
    .execute(&profile.profile_id, &request)?;
    assert_eq!(result.entries.len(), 2);
    Ok(())
}

#[test]
fn selectable_symlink_from_transport_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
    let profile = profile()?;
    let mut hostile = page(false)?;
    hostile.entries[1].selectable = true;
    hostile.entries[1].unavailable_reason = None;
    let store = Store(profile.clone());
    let browser = Browser { page: hostile };
    let result = BrowseRemoteDirectoryUseCase {
        profiles: &store,
        browser: &browser,
    }
    .execute(&profile.profile_id, &request(20)?);
    assert!(matches!(
        result,
        Err(BrowseRemoteDirectoryError::InvalidPage(_))
    ));
    Ok(())
}

#[test]
fn adapter_cannot_return_more_entries_than_requested() -> Result<(), Box<dyn std::error::Error>> {
    let profile = profile()?;
    let store = Store(profile.clone());
    let browser = Browser { page: page(false)? };
    let result = BrowseRemoteDirectoryUseCase {
        profiles: &store,
        browser: &browser,
    }
    .execute(&profile.profile_id, &request(1)?);
    assert!(matches!(
        result,
        Err(BrowseRemoteDirectoryError::InvalidPage(_))
    ));
    Ok(())
}

fn request(limit: u16) -> Result<RemoteBrowseRequest, Box<dyn std::error::Error>> {
    Ok(RemoteBrowseRequest {
        directory: RemotePath::parse("/srv")?,
        cursor: None,
        limit,
    })
}

fn page(truncated: bool) -> Result<RemoteBrowsePage, Box<dyn std::error::Error>> {
    Ok(RemoteBrowsePage {
        directory: RemotePath::parse("/srv")?,
        entries: vec![
            RemoteBrowseEntry {
                name: "app".to_owned(),
                absolute_path: RemotePath::parse("/srv/app")?,
                kind: RemoteEntryKind::Directory,
                size: None,
                modified_at: None,
                selectable: true,
                unavailable_reason: None,
            },
            RemoteBrowseEntry {
                name: "current".to_owned(),
                absolute_path: RemotePath::parse("/srv/current")?,
                kind: RemoteEntryKind::Symlink,
                size: None,
                modified_at: None,
                selectable: false,
                unavailable_reason: Some(RemoteEntryUnavailableReason::Symlink),
            },
        ],
        next_cursor: truncated.then(|| "next_1".to_owned()),
        truncated,
    })
}

fn profile() -> Result<VdsProfile, Box<dyn std::error::Error>> {
    Ok(VdsProfile {
        profile_id: ProfileId::parse("profile-001")?,
        label: "Server".to_owned(),
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

struct Browser {
    page: RemoteBrowsePage,
}

impl RemoteBrowserPort for Browser {
    fn list_directory(
        &self,
        _: &VdsProfile,
        _: &RemoteBrowseRequest,
    ) -> Result<RemoteBrowsePage, RemoteBrowserPortError> {
        Ok(self.page.clone())
    }
}
