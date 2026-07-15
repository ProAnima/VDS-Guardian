use crate::SshError;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use guardian_core::{CredentialId, SecretStore};
#[cfg(not(windows))]
use std::fs;
use std::{
    io::Write,
    path::{Path, PathBuf},
};
use tempfile::{Builder, NamedTempFile, TempPath};

const OPENSSH_HEADER: &str = "-----BEGIN OPENSSH PRIVATE KEY-----";
const OPENSSH_FOOTER: &str = "-----END OPENSSH PRIVATE KEY-----";
const PEM_RSA_HEADER: &str = "-----BEGIN RSA PRIVATE KEY-----";
const PEM_RSA_FOOTER: &str = "-----END RSA PRIVATE KEY-----";
const PEM_EC_HEADER: &str = "-----BEGIN EC PRIVATE KEY-----";
const PEM_EC_FOOTER: &str = "-----END EC PRIVATE KEY-----";
const PEM_PKCS8_HEADER: &str = "-----BEGIN PRIVATE KEY-----";
const PEM_PKCS8_FOOTER: &str = "-----END PRIVATE KEY-----";
const MAX_KEY_BYTES: usize = 64 * 1024;

/// Fixed first line of a self-describing marker stored under a
/// `CredentialId` in place of raw private-key bytes: it means "authenticate
/// through whatever OS SSH agent is already configured, presenting this
/// exact public identity" rather than "materialize this private key to a
/// temporary file." Never a secret by itself — only a public key.
const AGENT_MARKER_HEADER: &str = "AGENT-IDENTITY-V1";

const ALLOWED_AGENT_ALGORITHMS: [&str; 4] = [
    "ssh-ed25519",
    "ecdsa-sha2-nistp256",
    "ecdsa-sha2-nistp384",
    "ecdsa-sha2-nistp521",
];

/// A resolved SSH identity ready to hand to `-i` on the OpenSSH argv. Either
/// a short-lived private-key file (today's original path) or an
/// agent-backed identity: only a `.pub` sibling is ever written, with no
/// file at the identity path itself, relying on OpenSSH's own documented
/// fallback (read the public key from `<path>.pub`, then ask an already
/// running agent to sign with the matching loaded key) — the private key
/// bytes never reach this process or this disk at all.
pub enum SshIdentity {
    PrivateKey(TempPath),
    AgentPublicKey {
        pub_file: TempPath,
        identity_path: PathBuf,
    },
}

impl SshIdentity {
    pub fn from_store(store: &dyn SecretStore, id: &CredentialId) -> Result<Self, SshError> {
        let secret = store
            .load(id)
            .map_err(|_| SshError::CredentialUnavailable)?
            .ok_or(SshError::CredentialUnavailable)?;
        match classify_secret(secret.expose())? {
            Classified::PrivateKey => materialize_private_key(secret.expose()),
            Classified::AgentPublicKey {
                algorithm,
                public_key_base64,
            } => materialize_agent_identity(&algorithm, &public_key_base64),
        }
    }

    pub fn validate(bytes: &[u8]) -> Result<(), SshError> {
        classify_secret(bytes).map(|_| ())
    }

    /// Encodes a validated agent-identity marker for storage under a
    /// `CredentialId`, exactly as `from_store` later expects to read it —
    /// the only public entry point that produces marker bytes, so callers
    /// (the CLI's `credential register-agent-key` command) never need to
    /// know the marker's exact on-disk shape themselves.
    pub fn encode_agent_identity(
        algorithm: &str,
        public_key_base64: &str,
    ) -> Result<Vec<u8>, SshError> {
        valid_agent_public_key(algorithm, public_key_base64)
            .then(|| {
                format!("{AGENT_MARKER_HEADER}\n{algorithm}\n{public_key_base64}\n").into_bytes()
            })
            .ok_or(SshError::InvalidCredential)
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::PrivateKey(path) => path.as_ref(),
            Self::AgentPublicKey { identity_path, .. } => identity_path.as_path(),
        }
    }
}

enum Classified {
    PrivateKey,
    AgentPublicKey {
        algorithm: String,
        public_key_base64: String,
    },
}

fn classify_secret(bytes: &[u8]) -> Result<Classified, SshError> {
    if bytes.is_empty() || bytes.len() > MAX_KEY_BYTES || bytes.contains(&0) {
        return Err(SshError::InvalidCredential);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| SshError::InvalidCredential)?;
    let text = text.trim_end_matches(['\r', '\n']);
    if let Some((algorithm, public_key_base64)) = parse_agent_marker(text) {
        return valid_agent_public_key(algorithm, public_key_base64)
            .then(|| Classified::AgentPublicKey {
                algorithm: algorithm.to_owned(),
                public_key_base64: public_key_base64.to_owned(),
            })
            .ok_or(SshError::InvalidCredential);
    }
    if let Some(body) = pem_body(text, OPENSSH_HEADER, OPENSSH_FOOTER) {
        return validate_openssh_envelope(body).map(|()| Classified::PrivateKey);
    }
    for (header, footer) in [
        (PEM_RSA_HEADER, PEM_RSA_FOOTER),
        (PEM_EC_HEADER, PEM_EC_FOOTER),
        (PEM_PKCS8_HEADER, PEM_PKCS8_FOOTER),
    ] {
        if let Some(body) = pem_body(text, header, footer) {
            return validate_pem_private_key(body).map(|()| Classified::PrivateKey);
        }
    }
    Err(SshError::InvalidCredential)
}

fn parse_agent_marker(text: &str) -> Option<(&str, &str)> {
    let mut lines = text.lines();
    if lines.next() != Some(AGENT_MARKER_HEADER) {
        return None;
    }
    let algorithm = lines.next()?;
    let public_key_base64 = lines.next()?;
    lines
        .next()
        .is_none()
        .then_some((algorithm, public_key_base64))
}

/// Validates a base64 SSH public-key blob against RFC 4253 section 6.6's
/// length-prefixed-algorithm-name shape, mirroring the equivalent check in
/// `guardian_core::HostPin::validate`. Deliberately duplicated rather than
/// shared: this marker is guardian-ssh's own internal storage format and
/// never crosses into `guardian-core`/`VdsProfile`.
fn valid_agent_public_key(algorithm: &str, public_key_base64: &str) -> bool {
    if !ALLOWED_AGENT_ALGORITHMS.contains(&algorithm) {
        return false;
    }
    let Ok(decoded) = STANDARD.decode(public_key_base64.as_bytes()) else {
        return false;
    };
    let Some(length) = decoded
        .get(..4)
        .map(|bytes| u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize)
    else {
        return false;
    };
    decoded.get(4..4 + length) == Some(algorithm.as_bytes()) && decoded.len() > 4 + length
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

fn materialize_private_key(bytes: &[u8]) -> Result<SshIdentity, SshError> {
    let mut file = NamedTempFile::new().map_err(|_| SshError::TemporaryIdentityFile)?;
    file.write_all(bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|_| SshError::TemporaryIdentityFile)?;
    restrict_permissions(file.path())?;
    Ok(SshIdentity::PrivateKey(file.into_temp_path()))
}

/// Writes only a `.pub` file — never a private-key-shaped path — so
/// OpenSSH's own "no private key here, check `<path>.pub` and ask the
/// agent" fallback is what actually supplies the signature. Hardened with
/// the same permission restriction as a private-key file for consistency,
/// even though its content is not secret.
fn materialize_agent_identity(
    algorithm: &str,
    public_key_base64: &str,
) -> Result<SshIdentity, SshError> {
    let mut file = Builder::new()
        .suffix(".pub")
        .tempfile()
        .map_err(|_| SshError::TemporaryIdentityFile)?;
    let line = format!("{algorithm} {public_key_base64}\n");
    file.write_all(line.as_bytes())
        .and_then(|_| file.as_file().sync_all())
        .map_err(|_| SshError::TemporaryIdentityFile)?;
    restrict_permissions(file.path())?;
    let pub_file = file.into_temp_path();
    let pub_path: &Path = pub_file.as_ref();
    let identity_path = pub_path.with_extension("");
    Ok(SshIdentity::AgentPublicKey {
        pub_file,
        identity_path,
    })
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
        PEM_PKCS8_HEADER, PEM_RSA_FOOTER, PEM_RSA_HEADER, SshIdentity, classify_secret,
    };
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use guardian_core::{CredentialId, SecretStore, SecretStoreError, SecretValue};

    #[test]
    fn materialized_identity_is_deleted_on_drop() -> Result<(), Box<dyn std::error::Error>> {
        let id = CredentialId::parse("credential-001")?;
        let store = Store {
            secret: Some(SecretValue::new(valid_key())),
        };
        let identity = SshIdentity::from_store(&store, &id)?;
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
        let identity = SshIdentity::from_store(
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
        assert!(classify_secret(b"not a key").is_err());
        let encrypted = envelope(b"aes256-ctr", b"bcrypt", b"salt");
        assert!(classify_secret(&encrypted).is_err());
        assert!(classify_secret(b"-----BEGIN ENCRYPTED PRIVATE KEY-----\nAAAA\n-----END ENCRYPTED PRIVATE KEY-----\n").is_err());
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
            assert!(classify_secret(key.as_bytes()).is_ok());
        }
    }

    #[test]
    fn an_agent_marker_resolves_to_a_pub_only_identity_with_no_private_key_on_disk()
    -> Result<(), Box<dyn std::error::Error>> {
        let id = CredentialId::parse("credential-agent")?;
        let store = Store {
            secret: Some(SecretValue::new(SshIdentity::encode_agent_identity(
                "ssh-ed25519",
                &agent_public_key_blob(),
            )?)),
        };
        let identity = SshIdentity::from_store(&store, &id)?;
        let SshIdentity::AgentPublicKey {
            pub_file,
            identity_path,
        } = &identity
        else {
            return Err("expected an agent-backed identity".into());
        };
        assert!(!identity_path.exists(), "no private key file must exist");
        let pub_path: &std::path::Path = pub_file.as_ref();
        assert!(pub_path.exists());
        assert_eq!(pub_path.with_extension(""), *identity_path);
        let content = std::fs::read_to_string(pub_path)?;
        assert!(content.starts_with("ssh-ed25519 "));
        Ok(())
    }

    #[test]
    fn agent_markers_with_a_disallowed_algorithm_are_rejected() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&8_u32.to_be_bytes());
        payload.extend_from_slice(b"ssh-rsa");
        payload.push(1);
        let blob = STANDARD.encode(payload);
        let marker = format!("AGENT-IDENTITY-V1\nssh-rsa\n{blob}\n");
        assert!(classify_secret(marker.as_bytes()).is_err());
    }

    #[test]
    fn agent_markers_with_a_mismatched_embedded_algorithm_are_rejected() {
        let marker = format!("AGENT-IDENTITY-V1\nssh-ed25519\n{}\n", corrupt_blob());
        assert!(classify_secret(marker.as_bytes()).is_err());
    }

    #[test]
    fn agent_markers_with_the_wrong_line_count_are_rejected() {
        assert!(classify_secret(b"AGENT-IDENTITY-V1\nssh-ed25519\n").is_err());
        let too_many = format!(
            "AGENT-IDENTITY-V1\nssh-ed25519\n{}\nextra\n",
            agent_public_key_blob()
        );
        assert!(classify_secret(too_many.as_bytes()).is_err());
    }

    fn agent_public_key_blob() -> String {
        let mut payload = Vec::new();
        payload.extend_from_slice(&11_u32.to_be_bytes());
        payload.extend_from_slice(b"ssh-ed25519");
        payload.push(1);
        STANDARD.encode(payload)
    }

    fn corrupt_blob() -> String {
        // A different algorithm name embedded than the marker's own line
        // claims, so the cross-check in `valid_agent_public_key` fails.
        let mut payload = Vec::new();
        payload.extend_from_slice(&13_u32.to_be_bytes());
        payload.extend_from_slice(b"ecdsa-sha2-x");
        payload.push(1);
        STANDARD.encode(payload)
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
