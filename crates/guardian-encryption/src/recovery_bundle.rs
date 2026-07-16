//! Passphrase-protected export/import of a repository recovery key (ADR
//! 0013) into a portable, offline-copyable bundle. Reuses the same
//! self-describing AEAD envelope the rest of this crate already uses for
//! payload and vault-secret encryption — the only new primitive here is the
//! Argon2id key-derivation step that turns a human passphrase into a
//! wrapping key.

use crate::{PayloadKey, decrypt_self_describing_reader_to, encrypt_reader_to};
use argon2::{Algorithm, Argon2, Params, Version};
use rand_core::{OsRng, RngCore};
use std::io::Cursor;
use thiserror::Error;
use zeroize::Zeroizing;

pub const SALT_BYTES: usize = 16;
const DERIVED_KEY_BYTES: usize = 32;

/// Argon2id parameters for deriving a bundle's wrapping key from a
/// passphrase. Pinned explicitly rather than left to the `argon2` crate's
/// own changeable default, so the choice is a recorded ADR decision, not a
/// dependency-version accident. Generous by password-hashing standards
/// since this runs once per deliberate export/import, never on a routine
/// or login path — see ADR 0013 for the reasoning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl KdfParams {
    #[must_use]
    pub const fn recommended() -> Self {
        Self {
            m_cost: 65_536,
            t_cost: 3,
            p_cost: 4,
        }
    }
}

/// A recovery key wrapped under a passphrase-derived key. `salt` and the
/// chosen `KdfParams` are not secret; only `ciphertext` requires
/// confidentiality, and it is already self-authenticating (a wrong
/// passphrase or corrupt bundle fails the AEAD check, not a separate one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrappedRecoveryKey {
    pub salt: [u8; SALT_BYTES],
    pub ciphertext: Vec<u8>,
}

/// Wraps `key` (a repository recovery key) under a key derived from
/// `passphrase`, bound to `repository_id` via associated data so a bundle
/// exported from one repository can never be silently imported into
/// another with the same passphrase.
pub fn wrap_recovery_key(
    passphrase: &[u8],
    key: &PayloadKey,
    repository_id: &str,
    params: KdfParams,
) -> Result<WrappedRecoveryKey, RecoveryBundleError> {
    let mut salt = [0_u8; SALT_BYTES];
    OsRng.fill_bytes(&mut salt);
    let derived = derive_key(passphrase, &salt, params)?;
    let mut ciphertext = Vec::new();
    encrypt_reader_to(
        &derived,
        &mut Cursor::new(key.expose()),
        &mut ciphertext,
        &bundle_associated_data(repository_id),
    )
    .map_err(|_| RecoveryBundleError::Encryption)?;
    Ok(WrappedRecoveryKey { salt, ciphertext })
}

/// Reverses `wrap_recovery_key`. Fails closed via ordinary AEAD
/// authentication failure — collapsed into one error, deliberately not
/// distinguishing "wrong passphrase" from "wrong repository id" from
/// "corrupt bundle," since none of those distinctions are actionable
/// differently by a caller.
pub fn unwrap_recovery_key(
    passphrase: &[u8],
    wrapped: &WrappedRecoveryKey,
    repository_id: &str,
    params: KdfParams,
) -> Result<PayloadKey, RecoveryBundleError> {
    let derived = derive_key(passphrase, &wrapped.salt, params)?;
    let mut plaintext = Vec::new();
    decrypt_self_describing_reader_to(
        &derived,
        &mut Cursor::new(&wrapped.ciphertext),
        &mut plaintext,
        &bundle_associated_data(repository_id),
    )
    .map_err(|_| RecoveryBundleError::WrongPassphraseOrCorruptBundle)?;
    PayloadKey::from_bytes(&plaintext)
        .map_err(|_| RecoveryBundleError::WrongPassphraseOrCorruptBundle)
}

fn derive_key(
    passphrase: &[u8],
    salt: &[u8],
    params: KdfParams,
) -> Result<PayloadKey, RecoveryBundleError> {
    let argon2_params = Params::new(
        params.m_cost,
        params.t_cost,
        params.p_cost,
        Some(DERIVED_KEY_BYTES),
    )
    .map_err(|_| RecoveryBundleError::InvalidKdfParams)?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon2_params);
    let mut derived = Zeroizing::new([0_u8; DERIVED_KEY_BYTES]);
    argon2
        .hash_password_into(passphrase, salt, derived.as_mut_slice())
        .map_err(|_| RecoveryBundleError::InvalidKdfParams)?;
    PayloadKey::from_bytes(derived.as_slice()).map_err(|_| RecoveryBundleError::InvalidKdfParams)
}

fn bundle_associated_data(repository_id: &str) -> Vec<u8> {
    format!("guardian-recovery-bundle-v1|{repository_id}").into_bytes()
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RecoveryBundleError {
    #[error("recovery bundle key derivation parameters are invalid")]
    InvalidKdfParams,
    #[error("recovery bundle could not be sealed")]
    Encryption,
    #[error(
        "recovery bundle passphrase is incorrect, the repository id does not match, or the bundle is corrupt"
    )]
    WrongPassphraseOrCorruptBundle,
}

#[cfg(test)]
mod tests {
    use super::{KdfParams, RecoveryBundleError, unwrap_recovery_key, wrap_recovery_key};
    use crate::PayloadKey;

    fn fast_params() -> KdfParams {
        // Minimal viable Argon2id parameters -- keeps the test suite fast.
        // Production code always uses `KdfParams::recommended()`.
        KdfParams {
            m_cost: 8,
            t_cost: 1,
            p_cost: 1,
        }
    }

    #[test]
    fn wrapped_key_round_trips_with_the_right_passphrase() -> Result<(), RecoveryBundleError> {
        let key = PayloadKey::generate();
        let wrapped = wrap_recovery_key(
            b"correct horse battery staple",
            &key,
            "repo-001",
            fast_params(),
        )?;
        let recovered = unwrap_recovery_key(
            b"correct horse battery staple",
            &wrapped,
            "repo-001",
            fast_params(),
        )?;
        assert_eq!(recovered.expose(), key.expose());
        Ok(())
    }

    #[test]
    fn wrong_passphrase_fails_closed() -> Result<(), RecoveryBundleError> {
        let key = PayloadKey::generate();
        let wrapped = wrap_recovery_key(
            b"correct horse battery staple",
            &key,
            "repo-001",
            fast_params(),
        )?;
        assert_eq!(
            unwrap_recovery_key(b"wrong passphrase", &wrapped, "repo-001", fast_params()).err(),
            Some(RecoveryBundleError::WrongPassphraseOrCorruptBundle)
        );
        Ok(())
    }

    #[test]
    fn mismatched_repository_id_fails_closed() -> Result<(), RecoveryBundleError> {
        let key = PayloadKey::generate();
        let wrapped = wrap_recovery_key(
            b"correct horse battery staple",
            &key,
            "repo-001",
            fast_params(),
        )?;
        assert_eq!(
            unwrap_recovery_key(
                b"correct horse battery staple",
                &wrapped,
                "repo-002",
                fast_params()
            )
            .err(),
            Some(RecoveryBundleError::WrongPassphraseOrCorruptBundle)
        );
        Ok(())
    }

    #[test]
    fn tampered_ciphertext_fails_closed() -> Result<(), RecoveryBundleError> {
        let key = PayloadKey::generate();
        let mut wrapped = wrap_recovery_key(
            b"correct horse battery staple",
            &key,
            "repo-001",
            fast_params(),
        )?;
        let last = wrapped.ciphertext.len() - 1;
        wrapped.ciphertext[last] ^= 1;
        assert_eq!(
            unwrap_recovery_key(
                b"correct horse battery staple",
                &wrapped,
                "repo-001",
                fast_params()
            )
            .err(),
            Some(RecoveryBundleError::WrongPassphraseOrCorruptBundle)
        );
        Ok(())
    }
}
