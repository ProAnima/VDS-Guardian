//! Streaming AES-256-GCM payload envelopes.
//!
//! The format deliberately authenticates bounded chunks so a multi-gigabyte
//! archive never needs to be held in memory. Callers persist keys separately.

use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use rand_core::{OsRng, RngCore};
use std::io::{Read, Write};
use thiserror::Error;
use zeroize::Zeroizing;

pub const ENVELOPE_VERSION: u8 = 1;
pub const ALGORITHM: &str = "AES-256-GCM-CHUNKED";
pub const CHUNK_BYTES: usize = 1024 * 1024;
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;
const MAGIC: &[u8; 8] = b"VDSENC01";
const HEADER_BYTES: usize = MAGIC.len() + 1 + NONCE_BYTES;
const TAG_BYTES: usize = 16;

pub struct PayloadKey(Zeroizing<[u8; KEY_BYTES]>);

impl PayloadKey {
    #[must_use]
    pub fn generate() -> Self {
        let mut key = [0_u8; KEY_BYTES];
        OsRng.fill_bytes(&mut key);
        Self(Zeroizing::new(key))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, EncryptionError> {
        let key: [u8; KEY_BYTES] = bytes.try_into().map_err(|_| EncryptionError::InvalidKey)?;
        Ok(Self(Zeroizing::new(key)))
    }

    #[must_use]
    pub fn expose(&self) -> &[u8; KEY_BYTES] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvelopeHeader {
    pub version: u8,
    pub nonce: [u8; NONCE_BYTES],
}

pub fn encrypt_reader_to(
    key: &PayloadKey,
    input: &mut impl Read,
    output: &mut impl Write,
    associated_data: &[u8],
) -> Result<EnvelopeHeader, EncryptionError> {
    let mut nonce = [0_u8; NONCE_BYTES];
    OsRng.fill_bytes(&mut nonce);
    let header = EnvelopeHeader {
        version: ENVELOPE_VERSION,
        nonce,
    };
    output.write_all(MAGIC).map_err(|_| EncryptionError::Io)?;
    output
        .write_all(&[header.version])
        .map_err(|_| EncryptionError::Io)?;
    output
        .write_all(&header.nonce)
        .map_err(|_| EncryptionError::Io)?;
    let cipher = cipher(key)?;
    let mut buffer = vec![0_u8; CHUNK_BYTES];
    let mut index = 0_u32;
    loop {
        let read = input.read(&mut buffer).map_err(|_| EncryptionError::Io)?;
        if read == 0 {
            write_frame(
                &cipher,
                output,
                &header.nonce,
                index,
                true,
                &[],
                associated_data,
            )?;
            output.flush().map_err(|_| EncryptionError::Io)?;
            return Ok(header);
        }
        write_frame(
            &cipher,
            output,
            &header.nonce,
            index,
            false,
            &buffer[..read],
            associated_data,
        )?;
        index = index.checked_add(1).ok_or(EncryptionError::TooManyChunks)?;
    }
}

pub fn decrypt_reader_to(
    key: &PayloadKey,
    input: &mut impl Read,
    output: &mut impl Write,
    associated_data: &[u8],
    expected_nonce: &[u8; NONCE_BYTES],
) -> Result<(), EncryptionError> {
    let header = read_header(input)?;
    if &header.nonce != expected_nonce {
        return Err(EncryptionError::Failed);
    }
    let cipher = cipher(key)?;
    let mut index = 0_u32;
    loop {
        let (final_frame, ciphertext) = read_frame(input)?;
        let plaintext = decrypt_frame(
            &cipher,
            &header.nonce,
            index,
            final_frame,
            &ciphertext,
            associated_data,
        )?;
        if !final_frame {
            output
                .write_all(&plaintext)
                .map_err(|_| EncryptionError::Io)?;
            index = index.checked_add(1).ok_or(EncryptionError::TooManyChunks)?;
            continue;
        }
        if !plaintext.is_empty()
            || input
                .read(&mut [0_u8; 1])
                .map_err(|_| EncryptionError::Io)?
                != 0
        {
            return Err(EncryptionError::Failed);
        }
        output.flush().map_err(|_| EncryptionError::Io)?;
        return Ok(());
    }
}

fn cipher(key: &PayloadKey) -> Result<Aes256Gcm, EncryptionError> {
    Aes256Gcm::new_from_slice(key.expose()).map_err(|_| EncryptionError::InvalidKey)
}

fn write_frame(
    cipher: &Aes256Gcm,
    output: &mut impl Write,
    base_nonce: &[u8; NONCE_BYTES],
    index: u32,
    final_frame: bool,
    plaintext: &[u8],
    associated_data: &[u8],
) -> Result<(), EncryptionError> {
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&chunk_nonce(base_nonce, index)),
            Payload {
                msg: plaintext,
                aad: &frame_aad(associated_data, index, final_frame),
            },
        )
        .map_err(|_| EncryptionError::Failed)?;
    let length = u32::try_from(ciphertext.len()).map_err(|_| EncryptionError::TooLarge)?;
    output
        .write_all(&[u8::from(final_frame)])
        .map_err(|_| EncryptionError::Io)?;
    output
        .write_all(&length.to_be_bytes())
        .map_err(|_| EncryptionError::Io)?;
    output
        .write_all(&ciphertext)
        .map_err(|_| EncryptionError::Io)
}

fn decrypt_frame(
    cipher: &Aes256Gcm,
    base_nonce: &[u8; NONCE_BYTES],
    index: u32,
    final_frame: bool,
    ciphertext: &[u8],
    associated_data: &[u8],
) -> Result<Zeroizing<Vec<u8>>, EncryptionError> {
    cipher
        .decrypt(
            Nonce::from_slice(&chunk_nonce(base_nonce, index)),
            Payload {
                msg: ciphertext,
                aad: &frame_aad(associated_data, index, final_frame),
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| EncryptionError::Failed)
}

fn read_header(input: &mut impl Read) -> Result<EnvelopeHeader, EncryptionError> {
    let mut header = [0_u8; HEADER_BYTES];
    input
        .read_exact(&mut header)
        .map_err(|_| EncryptionError::Failed)?;
    if &header[..MAGIC.len()] != MAGIC || header[MAGIC.len()] != ENVELOPE_VERSION {
        return Err(EncryptionError::UnsupportedVersion);
    }
    let mut nonce = [0_u8; NONCE_BYTES];
    nonce.copy_from_slice(&header[MAGIC.len() + 1..]);
    Ok(EnvelopeHeader {
        version: ENVELOPE_VERSION,
        nonce,
    })
}

fn read_frame(input: &mut impl Read) -> Result<(bool, Vec<u8>), EncryptionError> {
    let mut final_frame = [0_u8; 1];
    input
        .read_exact(&mut final_frame)
        .map_err(|_| EncryptionError::Failed)?;
    if final_frame[0] > 1 {
        return Err(EncryptionError::Failed);
    }
    let mut length = [0_u8; 4];
    input
        .read_exact(&mut length)
        .map_err(|_| EncryptionError::Failed)?;
    let length =
        usize::try_from(u32::from_be_bytes(length)).map_err(|_| EncryptionError::TooLarge)?;
    if !(TAG_BYTES..=CHUNK_BYTES + TAG_BYTES).contains(&length) {
        return Err(EncryptionError::Failed);
    }
    let mut ciphertext = vec![0_u8; length];
    input
        .read_exact(&mut ciphertext)
        .map_err(|_| EncryptionError::Failed)?;
    Ok((final_frame[0] == 1, ciphertext))
}

fn chunk_nonce(base: &[u8; NONCE_BYTES], index: u32) -> [u8; NONCE_BYTES] {
    let mut nonce = *base;
    nonce[NONCE_BYTES - 4..].copy_from_slice(&index.to_be_bytes());
    nonce
}

fn frame_aad(associated_data: &[u8], index: u32, final_frame: bool) -> Vec<u8> {
    let mut aad = Vec::with_capacity(associated_data.len() + 5);
    aad.extend_from_slice(associated_data);
    aad.extend_from_slice(&index.to_be_bytes());
    aad.push(u8::from(final_frame));
    aad
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EncryptionError {
    #[error("payload key is invalid")]
    InvalidKey,
    #[error("payload envelope version is unsupported")]
    UnsupportedVersion,
    #[error("payload authentication failed")]
    Failed,
    #[error("payload is too large for the envelope format")]
    TooLarge,
    #[error("payload exceeds the envelope chunk limit")]
    TooManyChunks,
    #[error("payload envelope I/O failed")]
    Io,
}

#[cfg(test)]
mod tests {
    use super::{EncryptionError, PayloadKey, decrypt_reader_to, encrypt_reader_to};
    use std::io::Cursor;

    #[test]
    fn streaming_round_trip_authenticates_associated_data() -> Result<(), EncryptionError> {
        let key = PayloadKey::generate();
        let mut ciphertext = Vec::new();
        let header = encrypt_reader_to(
            &key,
            &mut Cursor::new(b"backup"),
            &mut ciphertext,
            b"backup-001|payload/filesystem.enc",
        )?;
        let mut plaintext = Vec::new();
        decrypt_reader_to(
            &key,
            &mut Cursor::new(ciphertext),
            &mut plaintext,
            b"backup-001|payload/filesystem.enc",
            &header.nonce,
        )?;
        assert_eq!(plaintext, b"backup");
        Ok(())
    }

    #[test]
    fn altered_data_and_aad_fail_closed() -> Result<(), EncryptionError> {
        let key = PayloadKey::generate();
        let mut ciphertext = Vec::new();
        let header = encrypt_reader_to(&key, &mut Cursor::new(b"backup"), &mut ciphertext, b"aad")?;
        let last = ciphertext.len() - 1;
        ciphertext[last] ^= 1;
        assert!(matches!(
            decrypt_reader_to(
                &key,
                &mut Cursor::new(ciphertext),
                &mut Vec::new(),
                b"aad",
                &header.nonce
            ),
            Err(EncryptionError::Failed)
        ));
        Ok(())
    }
}
