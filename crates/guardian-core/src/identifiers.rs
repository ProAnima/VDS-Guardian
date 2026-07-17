use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use thiserror::Error;

macro_rules! identifier {
    ($name:ident, $label:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, IdentifierError> {
                parse_identifier(value.into(), $label).map(Self)
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

identifier!(BackupId, "backup");
identifier!(RunId, "run");
identifier!(ProfileId, "profile");
identifier!(PlanId, "plan");
identifier!(RepositoryId, "repository");
identifier!(CredentialId, "credential");
identifier!(DatabaseId, "database");

impl RunId {
    /// Creates a sortable, random UUIDv7 correlation identifier for a new job.
    #[must_use]
    pub fn new() -> Self {
        Self(uuid::Uuid::now_v7().to_string())
    }
}

impl Default for RunId {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_identifier(value: String, kind: &'static str) -> Result<String, IdentifierError> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    valid
        .then_some(value)
        .ok_or(IdentifierError::Invalid { kind })
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct PayloadPath(String);

impl PayloadPath {
    pub fn parse(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        let segments_valid = value
            .split('/')
            .all(|segment| !segment.is_empty() && !matches!(segment, "." | ".."));
        let syntax_valid =
            !value.starts_with('/') && !value.contains(['\\', ':', '\0']) && value.len() <= 240;
        (segments_valid && syntax_valid)
            .then_some(Self(value))
            .ok_or(IdentifierError::InvalidPayloadPath)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ArchivePath(String);

impl ArchivePath {
    pub fn parse(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        let segments_valid = value
            .split('/')
            .all(|segment| !segment.is_empty() && !matches!(segment, "." | ".."));
        let syntax_valid =
            !value.starts_with('/') && !value.contains(['\\', ':', '\0']) && value.len() <= 1_024;
        (segments_valid && syntax_valid)
            .then_some(Self(value))
            .ok_or(IdentifierError::InvalidArchivePath)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ArchivePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

/// An absolute POSIX path on a *remote* deploy target host — deliberately not
/// a `PathBuf`, so it can never be validated with the local host's own path
/// semantics (relevant on Windows, where `Path::is_absolute()` means
/// something entirely different from "absolute on the remote Linux VDS").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct RemoteTargetPath(String);

impl RemoteTargetPath {
    pub fn parse(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        // The mirror image of PayloadPath's "must be relative": this path is
        // written on a remote host, so it must be absolute, and (unlike
        // guardian-ssh's capture-root validation) bare "/" is rejected too —
        // a deploy target is a path that must not already exist, and "/"
        // always does.
        let valid = value.starts_with('/')
            && value.len() <= 1_024
            && !value.contains(['\0', '\n', '\r', '\\'])
            && value
                .split('/')
                .skip(1)
                .all(|segment| !matches!(segment, "" | "." | ".."));
        valid
            .then_some(Self(value))
            .ok_or(IdentifierError::InvalidRemoteTargetPath)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A read-only absolute POSIX path on a remote Linux server. Unlike a deploy
/// target, root is valid because it is a useful starting point for browsing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct RemotePath(String);

impl RemotePath {
    pub fn parse(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        let valid = value == "/"
            || (value.starts_with('/')
                && value.len() <= 1_024
                && !value.contains(['\0', '\n', '\r', '\\'])
                && value
                    .split('/')
                    .skip(1)
                    .all(|segment| !matches!(segment, "" | "." | "..")));
        valid
            .then_some(Self(value))
            .ok_or(IdentifierError::InvalidRemotePath)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn child(&self, name: &str) -> Result<Self, IdentifierError> {
        let separator = if self.0 == "/" { "" } else { "/" };
        Self::parse(format!("{}{separator}{name}", self.0))
    }
}

impl<'de> Deserialize<'de> for RemotePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

impl<'de> Deserialize<'de> for RemoteTargetPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

impl<'de> Deserialize<'de> for PayloadPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Timestamp(String);

impl Timestamp {
    pub fn parse(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        is_utc_timestamp(&value)
            .then_some(Self(value))
            .ok_or(IdentifierError::InvalidTimestamp)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

fn is_utc_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || ![4, 7, 10, 13, 16, 19]
            .into_iter()
            .zip([b'-', b'-', b'T', b':', b':', b'Z'])
            .all(|(index, expected)| bytes[index] == expected)
        || !bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19) || byte.is_ascii_digit()
        })
    {
        return false;
    }
    let year = digits(bytes, 0, 4);
    let month = digits(bytes, 5, 2);
    let day = digits(bytes, 8, 2);
    let hour = digits(bytes, 11, 2);
    let minute = digits(bytes, 14, 2);
    let second = digits(bytes, 17, 2);
    match (year, month, day, hour, minute, second) {
        (Some(y), Some(m), Some(d), Some(h), Some(min), Some(sec)) => {
            y >= 1970
                && (1..=12).contains(&m)
                && d >= 1
                && d <= days_in_month(y, m)
                && h <= 23
                && min <= 59
                && sec <= 59
        }
        _ => false,
    }
}

fn digits(bytes: &[u8], start: usize, length: usize) -> Option<u32> {
    bytes
        .get(start..start + length)?
        .iter()
        .try_fold(0, |value, byte| {
            byte.is_ascii_digit()
                .then_some(value * 10 + u32::from(byte - b'0'))
        })
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        2 if year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IdentifierError {
    #[error("{kind} ID must contain 1-64 ASCII letters, digits, '-' or '_'")]
    Invalid { kind: &'static str },
    #[error("payload path must be a safe slash-separated relative path")]
    InvalidPayloadPath,
    #[error("archive path must be a safe slash-separated relative path")]
    InvalidArchivePath,
    #[error("remote target path must be a safe absolute POSIX path")]
    InvalidRemoteTargetPath,
    #[error("remote path must be a safe absolute POSIX path")]
    InvalidRemotePath,
    #[error("timestamp must use UTC second precision: YYYY-MM-DDTHH:MM:SSZ")]
    InvalidTimestamp,
}

#[cfg(test)]
mod tests {
    use super::{
        ArchivePath, BackupId, PayloadPath, RemotePath, RemoteTargetPath, RunId, Timestamp,
    };

    #[test]
    fn generated_run_ids_are_uuid_v7() {
        let value = RunId::new();
        assert_eq!(value.as_str().len(), 36);
        assert_eq!(&value.as_str()[14..15], "7");
        assert!(matches!(value.as_str().as_bytes()[19], b'8'..=b'b'));
        assert!(RunId::parse(value.as_str()).is_ok());
    }

    #[test]
    fn identifiers_reject_path_syntax() {
        assert!(BackupId::parse("backup_01").is_ok());
        assert!(BackupId::parse("../escape").is_err());
        assert!(BackupId::parse("C:\\backup").is_err());
    }

    #[test]
    fn payload_paths_fail_closed() {
        for hostile in ["../x", "/root", "C:/x", r"dir\x", "a//b", "./x"] {
            assert!(PayloadPath::parse(hostile).is_err(), "accepted {hostile}");
        }
        assert!(PayloadPath::parse("payload/filesystem-000.tar.zst").is_ok());
    }

    #[test]
    fn archive_paths_fail_closed() {
        for hostile in ["../x", "/root", "C:/x", r"dir\x", "a//b", "./x"] {
            assert!(ArchivePath::parse(hostile).is_err(), "accepted {hostile}");
        }
        assert!(ArchivePath::parse("srv/app/config.yaml").is_ok());
    }

    #[test]
    fn timestamps_require_canonical_utc_seconds() {
        assert!(Timestamp::parse("2026-07-13T12:00:00Z").is_ok());
        assert!(Timestamp::parse("2026-02-30T12:00:00Z").is_err());
        assert!(Timestamp::parse("2026-07-13T12:00:00+03:00").is_err());
    }

    #[test]
    fn deserialization_cannot_bypass_path_validation() {
        assert!(serde_json::from_str::<PayloadPath>(r#""../escape""#).is_err());
    }

    #[test]
    fn remote_target_paths_must_be_absolute_and_fail_closed() {
        for hostile in [
            "",
            "relative",
            "../x",
            "/",
            "//srv",
            "/srv/",
            "/srv/../x",
            "/srv/./x",
            "/srv\\app",
            "/srv\napp",
            "/srv\rapp",
            "/srv\0app",
        ] {
            assert!(
                RemoteTargetPath::parse(hostile).is_err(),
                "accepted {hostile:?}"
            );
        }
        assert!(RemoteTargetPath::parse("/srv/app").is_ok());
    }

    #[test]
    fn remote_browse_paths_allow_root_but_reject_traversal() {
        assert!(RemotePath::parse("/").is_ok());
        assert!(RemotePath::parse("/srv/app").is_ok());
        for hostile in ["relative", "/srv/../etc", "/srv//app", "/srv\napp"] {
            assert!(RemotePath::parse(hostile).is_err(), "accepted {hostile:?}");
        }
        let root = RemotePath::parse("/").unwrap_or_else(|error| unreachable!("{error}"));
        assert_eq!(
            root.child("srv")
                .unwrap_or_else(|error| unreachable!("{error}"))
                .as_str(),
            "/srv"
        );
    }
}
