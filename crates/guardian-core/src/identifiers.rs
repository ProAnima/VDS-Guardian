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
    #[error("timestamp must use UTC second precision: YYYY-MM-DDTHH:MM:SSZ")]
    InvalidTimestamp,
}

#[cfg(test)]
mod tests {
    use super::{BackupId, PayloadPath, Timestamp};

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
    fn timestamps_require_canonical_utc_seconds() {
        assert!(Timestamp::parse("2026-07-13T12:00:00Z").is_ok());
        assert!(Timestamp::parse("2026-02-30T12:00:00Z").is_err());
        assert!(Timestamp::parse("2026-07-13T12:00:00+03:00").is_err());
    }

    #[test]
    fn deserialization_cannot_bypass_path_validation() {
        assert!(serde_json::from_str::<PayloadPath>(r#""../escape""#).is_err());
    }
}
