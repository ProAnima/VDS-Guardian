use crate::{RepositoryError, filesystem::ensure_directory};
use guardian_core::Manifest;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_PAYLOAD_FILES: usize = 100_000;
const MAX_PAYLOAD_DEPTH: usize = 64;

pub(crate) fn sha256_bytes(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub(crate) fn verify_staged_payloads(
    payload_root: &Path,
    manifest: &Manifest,
) -> Result<(), RepositoryError> {
    manifest.validate_for_verification()?;
    verify_payload_tree(payload_root, manifest)
}

pub(crate) fn verify_sealed_payloads(
    payload_root: &Path,
    manifest: &Manifest,
) -> Result<(), RepositoryError> {
    manifest.validate_sealed()?;
    verify_payload_tree(payload_root, manifest)
}

fn verify_payload_tree(payload_root: &Path, manifest: &Manifest) -> Result<(), RepositoryError> {
    ensure_directory(payload_root)?;
    let actual = collect_payload_files(payload_root)?;
    let expected = manifest
        .payloads
        .iter()
        .map(|entry| PathBuf::from(entry.path.as_str()))
        .collect::<HashSet<_>>();
    if actual != expected {
        return Err(RepositoryError::IntegrityFailure);
    }
    for entry in &manifest.payloads {
        let path = payload_root.join(entry.path.as_str());
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|source| RepositoryError::io("inspect staged payload", source))?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() != entry.byte_length
        {
            return Err(RepositoryError::IntegrityFailure);
        }
        if hash_file(&path)? != entry.sha256.to_ascii_lowercase() {
            return Err(RepositoryError::IntegrityFailure);
        }
    }
    Ok(())
}

fn collect_payload_files(root: &Path) -> Result<HashSet<PathBuf>, RepositoryError> {
    let payload = root.join("payload");
    ensure_directory(&payload)?;
    let mut files = HashSet::new();
    walk_payload(root, &payload, 0, &mut files)?;
    Ok(files)
}

fn walk_payload(
    root: &Path,
    directory: &Path,
    depth: usize,
    files: &mut HashSet<PathBuf>,
) -> Result<(), RepositoryError> {
    if depth > MAX_PAYLOAD_DEPTH {
        return Err(RepositoryError::IntegrityFailure);
    }
    for entry in std::fs::read_dir(directory)
        .map_err(|source| RepositoryError::io("list staged payload", source))?
    {
        let entry =
            entry.map_err(|source| RepositoryError::io("read staged payload entry", source))?;
        let file_type = entry
            .file_type()
            .map_err(|source| RepositoryError::io("inspect staged payload type", source))?;
        if file_type.is_symlink() {
            return Err(RepositoryError::UnsafeFilesystemEntry);
        }
        if file_type.is_dir() {
            walk_payload(root, &entry.path(), depth + 1, files)?;
        } else if file_type.is_file() {
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|_| RepositoryError::UnsafeFilesystemEntry)?
                .to_path_buf();
            files.insert(relative);
            if files.len() > MAX_PAYLOAD_FILES {
                return Err(RepositoryError::IntegrityFailure);
            }
        } else {
            return Err(RepositoryError::UnsafeFilesystemEntry);
        }
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String, RepositoryError> {
    let mut file =
        File::open(path).map_err(|source| RepositoryError::io("open staged payload", source))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|source| RepositoryError::io("hash staged payload", source))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(hex(&digest.finalize()))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(ALPHABET[usize::from(byte >> 4)]));
        output.push(char::from(ALPHABET[usize::from(byte & 0x0f)]));
    }
    output
}
