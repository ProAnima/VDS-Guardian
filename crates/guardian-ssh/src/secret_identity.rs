use crate::SshError;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use guardian_core::{CredentialId, SecretStore};
#[cfg(not(windows))]
use std::fs;
use std::{io::Write, path::Path};
use tempfile::{NamedTempFile, TempPath};

const OPENSSH_HEADER: &str = "-----BEGIN OPENSSH PRIVATE KEY-----";
const OPENSSH_FOOTER: &str = "-----END OPENSSH PRIVATE KEY-----";
const PEM_RSA_HEADER: &str = "-----BEGIN RSA PRIVATE KEY-----";
const PEM_RSA_FOOTER: &str = "-----END RSA PRIVATE KEY-----";
const PEM_EC_HEADER: &str = "-----BEGIN EC PRIVATE KEY-----";
const PEM_EC_FOOTER: &str = "-----END EC PRIVATE KEY-----";
const PEM_PKCS8_HEADER: &str = "-----BEGIN PRIVATE KEY-----";
const PEM_PKCS8_FOOTER: &str = "-----END PRIVATE KEY-----";
const MAX_KEY_BYTES: usize = 64 * 1024;

pub struct SecretIdentityFile {
    path: TempPath,
}

impl SecretIdentityFile {
    pub fn from_store(store: &dyn SecretStore, id: &CredentialId) -> Result<Self, SshError> {
        let secret = store
            .load(id)
            .map_err(|_| SshError::CredentialUnavailable)?
            .ok_or(SshError::CredentialUnavailable)?;
        validate_private_key(secret.expose())?;
        let mut file = NamedTempFile::new().map_err(|_| SshError::TemporaryIdentityFile)?;
        file.write_all(secret.expose())
            .and_then(|_| file.as_file().sync_all())
            .map_err(|_| SshError::TemporaryIdentityFile)?;
        restrict_permissions(file.path())?;
        Ok(Self {
            path: file.into_temp_path(),
        })
    }

    pub fn validate(bytes: &[u8]) -> Result<(), SshError> {
        validate_private_key(bytes)
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        self.path.as_ref()
    }
}

fn validate_private_key(bytes: &[u8]) -> Result<(), SshError> {
    if bytes.is_empty() || bytes.len() > MAX_KEY_BYTES || bytes.contains(&0) {
        return Err(SshError::InvalidCredential);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| SshError::InvalidCredential)?;
    let text = text.trim_end_matches(['\r', '\n']);
    if let Some(body) = pem_body(text, OPENSSH_HEADER, OPENSSH_FOOTER) {
        return validate_openssh_envelope(body);
    }
    for (header, footer) in [
        (PEM_RSA_HEADER, PEM_RSA_FOOTER),
        (PEM_EC_HEADER, PEM_EC_FOOTER),
        (PEM_PKCS8_HEADER, PEM_PKCS8_FOOTER),
    ] {
        if let Some(body) = pem_body(text, header, footer) {
            return validate_pem_private_key(body);
        }
    }
    Err(SshError::InvalidCredential)
}

fn pem_body<'a>(text: &'a str, header: &str, footer: &str) -> Option<&'a str> {
    text.strip_prefix(header)
        .and_then(|value| {
            value
                .strip_prefix("\r\n")
                .or_else(|| value.strip_prefix('\n'))
        })
        .and_then(|value| value.strip_suffix(footer))
}

fn validate_openssh_envelope(body: &str) -> Result<(), SshError> {
    let encoded: String = body.lines().collect();
    let decoded = STANDARD
        .decode(encoded.as_bytes())
        .map_err(|_| SshError::InvalidCredential)?;
    let mut cursor = decoded.as_slice();
    if !take_prefix(&mut cursor, b"openssh-key-v1\0")
        || read_string(&mut cursor) != Some(b"none".as_slice())
        || read_string(&mut cursor) != Some(b"none".as_slice())
        || read_string(&mut cursor) != Some(b"".as_slice())
    {
        return Err(SshError::InvalidCredential);
    }
    Ok(())
}

fn validate_pem_private_key(body: &str) -> Result<(), SshError> {
    let encoded: String = body.lines().collect();
    let der = STANDARD
        .decode(encoded.as_bytes())
        .map_err(|_| SshError::InvalidCredential)?;
    (der.len() > 8 && der.first() == Some(&0x30))
        .then_some(())
        .ok_or(SshError::InvalidCredential)
}

fn take_prefix(cursor: &mut &[u8], prefix: &[u8]) -> bool {
    let Some(rest) = cursor.strip_prefix(prefix) else {
        return false;
    };
    *cursor = rest;
    true
}

fn read_string<'a>(cursor: &mut &'a [u8]) -> Option<&'a [u8]> {
    let length = usize::try_from(u32::from_be_bytes(cursor.get(..4)?.try_into().ok()?)).ok()?;
    let value = cursor.get(4..4 + length)?;
    *cursor = cursor.get(4 + length..)?;
    Some(value)
}

pub(crate) fn restrict_permissions(path: &Path) -> Result<(), SshError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|_| SshError::TemporaryIdentityFile)?;
        return Ok(());
    }
    #[cfg(windows)]
    {
        restrict_windows_permissions(path)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let metadata = fs::metadata(path).map_err(|_| SshError::TemporaryIdentityFile)?;
        if !metadata.is_file() {
            return Err(SshError::TemporaryIdentityFile);
        }
        Ok(())
    }
}

#[cfg(windows)]
fn system32_binary(name: &str) -> std::path::PathBuf {
    let mut path = std::path::PathBuf::from(
        std::env::var_os("SystemRoot").unwrap_or_else(|| std::ffi::OsString::from(r"C:\Windows")),
    );
    path.push("System32");
    path.push(name);
    path
}

#[cfg(windows)]
fn restrict_windows_permissions(path: &Path) -> Result<(), SshError> {
    let identity = std::process::Command::new(system32_binary("whoami.exe"))
        .arg("/user")
        .output()
        .map_err(|_| SshError::TemporaryIdentityFile)?;
    if !identity.status.success() {
        return Err(SshError::TemporaryIdentityFile);
    }
    // whoami's table header is localized OEM-codepage text on non-English
    // Windows installs and is not valid UTF-8; decode losslessly and search
    // for the SID token, which is always plain ASCII regardless of locale.
    let sid = String::from_utf8_lossy(&identity.stdout)
        .split_ascii_whitespace()
        .find(|part| part.starts_with("S-1-"))
        .map(str::to_owned)
        .ok_or(SshError::TemporaryIdentityFile)?;
    let hardened = std::process::Command::new(system32_binary("icacls.exe"))
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("*{sid}:F"))
        .arg("/c")
        .output()
        .map_err(|_| SshError::TemporaryIdentityFile)?;
    hardened
        .status
        .success()
        .then_some(())
        .ok_or(SshError::TemporaryIdentityFile)
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::system32_binary;
    use super::{
        OPENSSH_FOOTER, OPENSSH_HEADER, PEM_EC_FOOTER, PEM_EC_HEADER, PEM_PKCS8_FOOTER,
        PEM_PKCS8_HEADER, PEM_RSA_FOOTER, PEM_RSA_HEADER, SecretIdentityFile, validate_private_key,
    };
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use guardian_core::{CredentialId, SecretStore, SecretStoreError, SecretValue};

    #[test]
    fn materialized_identity_is_deleted_on_drop() -> Result<(), Box<dyn std::error::Error>> {
        let id = CredentialId::parse("credential-001")?;
        let store = Store {
            secret: Some(SecretValue::new(valid_key())),
        };
        let identity = SecretIdentityFile::from_store(&store, &id)?;
        let path = identity.path().to_owned();
        assert!(path.is_file());
        drop(identity);
        assert!(!path.exists());
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn materialized_identity_removes_inherited_windows_permissions()
    -> Result<(), Box<dyn std::error::Error>> {
        let id = CredentialId::parse("credential-001")?;
        let identity = SecretIdentityFile::from_store(
            &Store {
                secret: Some(SecretValue::new(valid_key())),
            },
            &id,
        )?;
        let acl = std::process::Command::new(system32_binary("icacls.exe"))
            .arg(identity.path())
            .output()?;
        let rendered = String::from_utf8_lossy(&acl.stdout).into_owned();
        assert!(acl.status.success());
        assert!(!rendered.contains("(I)"));
        Ok(())
    }

    #[test]
    fn encrypted_or_malformed_key_envelopes_fail_closed() {
        assert!(validate_private_key(b"not a key").is_err());
        let encrypted = envelope(b"aes256-ctr", b"bcrypt", b"salt");
        assert!(validate_private_key(&encrypted).is_err());
        assert!(validate_private_key(b"-----BEGIN ENCRYPTED PRIVATE KEY-----\nAAAA\n-----END ENCRYPTED PRIVATE KEY-----\n").is_err());
    }

    #[test]
    fn unencrypted_common_pem_keys_are_accepted() {
        let der = [
            0x30, 0x09, 0x02, 0x01, 0x00, 0x02, 0x04, 0x01, 0x02, 0x03, 0x04,
        ];
        for (header, footer) in [
            (PEM_RSA_HEADER, PEM_RSA_FOOTER),
            (PEM_EC_HEADER, PEM_EC_FOOTER),
            (PEM_PKCS8_HEADER, PEM_PKCS8_FOOTER),
        ] {
            let key = format!("{header}\r\n{}\r\n{footer}\r\n", STANDARD.encode(der));
            assert!(validate_private_key(key.as_bytes()).is_ok());
        }
    }

    fn valid_key() -> Vec<u8> {
        envelope(b"none", b"none", b"")
    }

    fn envelope(cipher: &[u8], kdf: &[u8], options: &[u8]) -> Vec<u8> {
        let mut bytes = b"openssh-key-v1\0".to_vec();
        for value in [cipher, kdf, options] {
            bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
            bytes.extend_from_slice(value);
        }
        let encoded = STANDARD.encode(bytes);
        format!("{OPENSSH_HEADER}\n{encoded}\n{OPENSSH_FOOTER}\n").into_bytes()
    }

    struct Store {
        secret: Option<SecretValue>,
    }

    impl SecretStore for Store {
        fn load(&self, _: &CredentialId) -> Result<Option<SecretValue>, SecretStoreError> {
            Ok(self
                .secret
                .as_ref()
                .map(|value| SecretValue::new(value.expose().to_vec())))
        }

        fn store(&self, _: &CredentialId, _: &SecretValue) -> Result<(), SecretStoreError> {
            Ok(())
        }

        fn delete(&self, _: &CredentialId) -> Result<(), SecretStoreError> {
            Ok(())
        }
    }
}
