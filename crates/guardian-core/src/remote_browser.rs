use crate::{
    ProfileId, ProfileStorePort, ProfileStorePortError, RemotePath, Timestamp, VdsProfile,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

pub const MAX_REMOTE_PAGE_ENTRIES: usize = 200;

pub trait RemoteBrowserPort: Send + Sync {
    fn list_directory(
        &self,
        profile: &VdsProfile,
        request: &RemoteBrowseRequest,
    ) -> Result<RemoteBrowsePage, RemoteBrowserPortError>;
}

pub struct BrowseRemoteDirectoryUseCase<'a> {
    pub profiles: &'a dyn ProfileStorePort,
    pub browser: &'a dyn RemoteBrowserPort,
}

impl BrowseRemoteDirectoryUseCase<'_> {
    pub fn execute(
        &self,
        profile_id: &ProfileId,
        request: &RemoteBrowseRequest,
    ) -> Result<RemoteBrowsePage, BrowseRemoteDirectoryError> {
        request
            .validate()
            .map_err(BrowseRemoteDirectoryError::InvalidRequest)?;
        let profile = self
            .profiles
            .get(profile_id)
            .map_err(BrowseRemoteDirectoryError::ProfileStore)?
            .ok_or(BrowseRemoteDirectoryError::ProfileNotFound)?;
        let page = self
            .browser
            .list_directory(&profile, request)
            .map_err(BrowseRemoteDirectoryError::Browser)?;
        page.validate_for(request)
            .map_err(BrowseRemoteDirectoryError::InvalidPage)?;
        Ok(page)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RemoteBrowseRequest {
    pub directory: RemotePath,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub limit: u16,
}

impl RemoteBrowseRequest {
    pub fn validate(&self) -> Result<(), RemoteBrowseError> {
        let cursor_valid = self.cursor.as_deref().is_none_or(valid_cursor);
        (usize::from(self.limit) > 0
            && usize::from(self.limit) <= MAX_REMOTE_PAGE_ENTRIES
            && cursor_valid)
            .then_some(())
            .ok_or(RemoteBrowseError::InvalidRequest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RemoteBrowsePage {
    pub directory: RemotePath,
    pub entries: Vec<RemoteBrowseEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub truncated: bool,
}

impl RemoteBrowsePage {
    pub fn validate_for(&self, request: &RemoteBrowseRequest) -> Result<(), RemoteBrowseError> {
        if self.directory != request.directory
            || self.entries.len() > usize::from(request.limit)
            || self
                .next_cursor
                .as_deref()
                .is_some_and(|value| !valid_cursor(value))
            || (self.truncated != self.next_cursor.is_some())
        {
            return Err(RemoteBrowseError::InvalidPage);
        }
        let mut paths = BTreeSet::new();
        for entry in &self.entries {
            entry.validate(&self.directory)?;
            if !paths.insert(entry.absolute_path.as_str()) {
                return Err(RemoteBrowseError::DuplicateEntry);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RemoteBrowseEntry {
    pub name: String,
    pub absolute_path: RemotePath,
    pub kind: RemoteEntryKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<Timestamp>,
    pub selectable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<RemoteEntryUnavailableReason>,
}

impl RemoteBrowseEntry {
    fn validate(&self, parent: &RemotePath) -> Result<(), RemoteBrowseError> {
        let valid_name = !self.name.is_empty()
            && self.name.len() <= 255
            && !self.name.chars().any(char::is_control)
            && !self.name.contains(['/', '\\']);
        let expected = parent
            .child(&self.name)
            .map_err(|_| RemoteBrowseError::InvalidEntry)?;
        let selection_valid = match self.kind {
            RemoteEntryKind::Directory | RemoteEntryKind::RegularFile => {
                self.selectable && self.unavailable_reason.is_none()
            }
            RemoteEntryKind::Symlink | RemoteEntryKind::Other => {
                !self.selectable && self.unavailable_reason.is_some()
            }
        };
        (valid_name && expected == self.absolute_path && selection_valid)
            .then_some(())
            .ok_or(RemoteBrowseError::InvalidEntry)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteEntryKind {
    Directory,
    RegularFile,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteEntryUnavailableReason {
    Symlink,
    SpecialFile,
}

fn valid_cursor(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum RemoteBrowserPortError {
    #[error("remote browser input or output was rejected")]
    Rejected,
    #[error("remote browser is unavailable")]
    Unavailable,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum RemoteBrowseError {
    #[error("remote browse request is invalid")]
    InvalidRequest,
    #[error("remote browse page is invalid")]
    InvalidPage,
    #[error("remote browse entry is invalid")]
    InvalidEntry,
    #[error("remote browse page contains a duplicate entry")]
    DuplicateEntry,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BrowseRemoteDirectoryError {
    #[error("server profile was not found")]
    ProfileNotFound,
    #[error(transparent)]
    ProfileStore(#[from] ProfileStorePortError),
    #[error(transparent)]
    Browser(#[from] RemoteBrowserPortError),
    #[error(transparent)]
    InvalidRequest(RemoteBrowseError),
    #[error(transparent)]
    InvalidPage(RemoteBrowseError),
}
