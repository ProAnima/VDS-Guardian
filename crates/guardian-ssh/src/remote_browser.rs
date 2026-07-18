use super::{PinnedHost, SshIdentity, SshUser, SystemOpenSsh, shell_quote};
use guardian_core::{
    RemoteBrowseEntry, RemoteBrowsePage, RemoteBrowseRequest, RemoteBrowserPort,
    RemoteBrowserPortError, RemoteEntryKind, RemoteEntryUnavailableReason, RemotePath, SecretStore,
    VdsProfile,
};
use sha2::{Digest, Sha256};
use std::fs;
use tempfile::tempdir;

const MAX_BROWSE_OUTPUT_BYTES: u64 = 1024 * 1024;

pub struct SshRemoteBrowserAdapter<'a> {
    pub ssh: &'a SystemOpenSsh,
    pub credentials: &'a dyn SecretStore,
}

impl RemoteBrowserPort for SshRemoteBrowserAdapter<'_> {
    fn list_directory(
        &self,
        profile: &VdsProfile,
        request: &RemoteBrowseRequest,
    ) -> Result<RemoteBrowsePage, RemoteBrowserPortError> {
        profile
            .validate()
            .map_err(|_| RemoteBrowserPortError::Rejected)?;
        request
            .validate()
            .map_err(|_| RemoteBrowserPortError::Rejected)?;
        let host = pinned_host(profile)?;
        let user =
            SshUser::parse(&profile.endpoint.user).map_err(|_| RemoteBrowserPortError::Rejected)?;
        let identity = SshIdentity::from_store(self.credentials, &profile.credential_id)
            .map_err(|_| RemoteBrowserPortError::Unavailable)?;
        let temporary = tempdir().map_err(|_| RemoteBrowserPortError::Unavailable)?;
        let destination = temporary.path().join("directory-listing.bin");
        self.ssh
            .browse_directory_to(
                &host,
                &user,
                identity.path(),
                &request.directory,
                &destination,
                MAX_BROWSE_OUTPUT_BYTES,
            )
            .map_err(|_| RemoteBrowserPortError::Unavailable)?;
        let bytes = fs::read(destination).map_err(|_| RemoteBrowserPortError::Unavailable)?;
        parse_listing(&bytes, request).map_err(|_| RemoteBrowserPortError::Rejected)
    }
}

fn pinned_host(profile: &VdsProfile) -> Result<PinnedHost, RemoteBrowserPortError> {
    PinnedHost::parse(
        &profile.endpoint.host,
        profile.endpoint.port,
        &profile.endpoint.host_pin.algorithm,
        &profile.endpoint.host_pin.public_key_base64,
    )
    .map_err(|_| RemoteBrowserPortError::Rejected)
}

pub(crate) fn browse_command(directory: &RemotePath) -> String {
    format!(
        "LC_ALL=C find {} -mindepth 1 -maxdepth 1 -printf '%y %s %TY-%Tm-%TdT%TH:%TM:%.2TSZ %f\\0'",
        shell_quote(directory.as_str())
    )
}

fn parse_listing(
    bytes: &[u8],
    request: &RemoteBrowseRequest,
) -> Result<RemoteBrowsePage, RemoteBrowserPortError> {
    if bytes.len() > usize::try_from(MAX_BROWSE_OUTPUT_BYTES).unwrap_or(usize::MAX) {
        return Err(RemoteBrowserPortError::Rejected);
    }
    let mut entries = parse_records(bytes, &request.directory)?;
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    let digest = listing_digest(&entries);
    let offset = parse_cursor(request.cursor.as_deref(), &digest)?;
    if offset > entries.len() {
        return Err(RemoteBrowserPortError::Rejected);
    }
    let end = offset
        .saturating_add(usize::from(request.limit))
        .min(entries.len());
    let truncated = end < entries.len();
    let page = RemoteBrowsePage {
        directory: request.directory.clone(),
        entries: entries[offset..end].to_vec(),
        next_cursor: truncated.then(|| format!("v1_{end}_{digest}")),
        truncated,
    };
    page.validate_for(request)
        .map_err(|_| RemoteBrowserPortError::Rejected)?;
    Ok(page)
}

fn listing_digest(entries: &[RemoteBrowseEntry]) -> String {
    let mut hasher = Sha256::new();
    for entry in entries {
        hasher.update(entry.name.as_bytes());
        hasher.update([0]);
        hasher.update(entry.absolute_path.as_str().as_bytes());
        hasher.update([0, entry.kind as u8, u8::from(entry.selectable)]);
        hasher.update(entry.size.unwrap_or_default().to_be_bytes());
        if let Some(modified_at) = &entry.modified_at {
            hasher.update(modified_at.as_str().as_bytes());
        }
    }
    hasher
        .finalize()
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn parse_cursor(
    cursor: Option<&str>,
    expected_digest: &str,
) -> Result<usize, RemoteBrowserPortError> {
    let Some(cursor) = cursor else { return Ok(0) };
    let mut fields = cursor.split('_');
    let valid_version = fields.next() == Some("v1");
    let offset = fields
        .next()
        .ok_or(RemoteBrowserPortError::Rejected)?
        .parse::<usize>()
        .map_err(|_| RemoteBrowserPortError::Rejected)?;
    let digest_matches = fields.next() == Some(expected_digest);
    if valid_version && digest_matches && fields.next().is_none() {
        Ok(offset)
    } else {
        Err(RemoteBrowserPortError::Rejected)
    }
}

fn parse_records(
    bytes: &[u8],
    parent: &RemotePath,
) -> Result<Vec<RemoteBrowseEntry>, RemoteBrowserPortError> {
    if !bytes.is_empty() && !bytes.ends_with(&[0]) {
        return Err(RemoteBrowserPortError::Rejected);
    }
    bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .map(|record| parse_record(record, parent))
        .collect()
}

fn parse_record(
    record: &[u8],
    parent: &RemotePath,
) -> Result<RemoteBrowseEntry, RemoteBrowserPortError> {
    let rendered = std::str::from_utf8(record).map_err(|_| RemoteBrowserPortError::Rejected)?;
    let mut fields = rendered.splitn(4, ' ');
    let kind = fields.next().ok_or(RemoteBrowserPortError::Rejected)?;
    let size = fields
        .next()
        .ok_or(RemoteBrowserPortError::Rejected)?
        .parse::<u64>()
        .map_err(|_| RemoteBrowserPortError::Rejected)?;
    let modified_at = fields
        .next()
        .ok_or(RemoteBrowserPortError::Rejected)
        .and_then(|value| {
            guardian_core::Timestamp::parse(value).map_err(|_| RemoteBrowserPortError::Rejected)
        })?;
    let name = fields.next().ok_or(RemoteBrowserPortError::Rejected)?;
    let absolute_path = parent
        .child(name)
        .map_err(|_| RemoteBrowserPortError::Rejected)?;
    let (kind, selectable, unavailable_reason) = entry_kind(kind)?;
    Ok(RemoteBrowseEntry {
        name: name.to_owned(),
        absolute_path,
        kind,
        size: (kind == RemoteEntryKind::RegularFile).then_some(size),
        modified_at: Some(modified_at),
        selectable,
        unavailable_reason,
    })
}

fn entry_kind(
    value: &str,
) -> Result<(RemoteEntryKind, bool, Option<RemoteEntryUnavailableReason>), RemoteBrowserPortError> {
    match value {
        "d" => Ok((RemoteEntryKind::Directory, true, None)),
        "f" => Ok((RemoteEntryKind::RegularFile, true, None)),
        "l" => Ok((
            RemoteEntryKind::Symlink,
            false,
            Some(RemoteEntryUnavailableReason::Symlink),
        )),
        value if value.len() == 1 => Ok((
            RemoteEntryKind::Other,
            false,
            Some(RemoteEntryUnavailableReason::SpecialFile),
        )),
        _ => Err(RemoteBrowserPortError::Rejected),
    }
}

#[cfg(test)]
mod tests {
    use super::{browse_command, parse_listing};
    use guardian_core::{RemoteBrowseRequest, RemoteEntryKind, RemotePath};

    #[test]
    fn command_quotes_directory_and_remains_read_only() -> Result<(), Box<dyn std::error::Error>> {
        let path = RemotePath::parse("/srv/app's data")?;
        let command = browse_command(&path);
        assert!(command.contains("'/srv/app'\"'\"'s data'"));
        assert!(command.starts_with("LC_ALL=C find "));
        assert!(!command.contains("rm "));
        Ok(())
    }

    #[test]
    fn listing_is_sorted_paginated_and_does_not_select_symlinks()
    -> Result<(), Box<dyn std::error::Error>> {
        let request = RemoteBrowseRequest {
            directory: RemotePath::parse("/srv")?,
            cursor: None,
            limit: 2,
        };
        let listing = b"l 7 2026-07-18T10:00:00Z current\0f 42 2026-07-18T10:01:00Z z.txt\0d 0 2026-07-18T09:59:00Z app\0";
        let page = parse_listing(listing, &request)?;
        assert_eq!(page.entries[0].name, "app");
        assert_eq!(page.entries[1].kind, RemoteEntryKind::Symlink);
        assert!(!page.entries[1].selectable);
        let cursor = page
            .next_cursor
            .clone()
            .ok_or_else(|| std::io::Error::other("truncated page cursor is missing"))?;
        assert!(cursor.starts_with("v1_2_"));
        let next = parse_listing(
            listing,
            &RemoteBrowseRequest {
                cursor: Some(cursor),
                ..request.clone()
            },
        )?;
        assert_eq!(next.entries[0].name, "z.txt");
        assert!(!next.truncated);
        Ok(())
    }

    #[test]
    fn cursor_is_rejected_when_the_listing_changes() -> Result<(), Box<dyn std::error::Error>> {
        let request = RemoteBrowseRequest {
            directory: RemotePath::parse("/srv")?,
            cursor: None,
            limit: 1,
        };
        let first = parse_listing(
            b"d 0 2026-07-18T10:00:00Z app\0f 42 2026-07-18T10:00:00Z z.txt\0",
            &request,
        )?;
        let stale = RemoteBrowseRequest {
            cursor: first.next_cursor,
            ..request
        };
        assert!(
            parse_listing(
                b"d 0 2026-07-18T10:00:00Z app\0f 43 2026-07-18T10:00:00Z z.txt\0",
                &stale,
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn malformed_or_traversing_names_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let request = RemoteBrowseRequest {
            directory: RemotePath::parse("/srv")?,
            cursor: None,
            limit: 20,
        };
        assert!(parse_listing(b"f 1 2026-07-18T10:00:00Z ../etc\0", &request).is_err());
        assert!(parse_listing(b"f nope 2026-07-18T10:00:00Z file\0", &request).is_err());
        assert!(parse_listing(b"f 1 invalid file\0", &request).is_err());
        assert!(parse_listing(b"f 1 2026-07-18T10:00:00Z unterminated", &request).is_err());
        Ok(())
    }
}
