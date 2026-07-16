use guardian_archive::{ArchiveError, ArchiveLimits, inspect_tar_zstd};
use std::io;
use tar::EntryType;

#[test]
fn inspect_accepts_regular_files_and_directories() -> Result<(), Box<dyn std::error::Error>> {
    // Real tar writers (GNU tar, BSD tar) always suffix a directory member's
    // own name with `/` on the wire — this is deliberately reproduced here,
    // not a typo, since a hand-built archive without it would silently miss
    // testing the shape every real capture actually produces.
    let archive = archive(&[
        ("srv/app/", EntryType::Directory, b""),
        ("srv/app/config.yaml", EntryType::Regular, b"safe"),
    ])?;
    let inspection = inspect_tar_zstd(archive.as_slice(), ArchiveLimits::conservative())?;
    assert_eq!(inspection.entries, 2);
    assert_eq!(inspection.directories, 1);
    assert_eq!(inspection.regular_files, 1);
    assert!(inspection.expanded_bytes >= 3);
    Ok(())
}

#[test]
fn inspection_rejects_hostile_paths_and_link_entries() -> Result<(), Box<dyn std::error::Error>> {
    for (path, kind) in [
        ("../escape", EntryType::Regular),
        ("C:/Windows", EntryType::Regular),
        ("safe/link", EntryType::Symlink),
        ("safe/hard-link", EntryType::Link),
        // A trailing slash is only tolerated for a real directory entry
        // (see `inspect_accepts_regular_files_and_directories`) — a file
        // entry claiming one is not real tar output and stays rejected.
        ("safe/trailing-slash/", EntryType::Regular),
    ] {
        let archive = archive(&[(path, kind, b"content")])?;
        assert!(matches!(
            inspect_tar_zstd(archive.as_slice(), ArchiveLimits::conservative()),
            Err(ArchiveError::UnsafePath | ArchiveError::UnsupportedEntryType)
        ));
    }
    Ok(())
}

#[test]
fn inspection_enforces_entry_and_size_limits() -> Result<(), Box<dyn std::error::Error>> {
    let archive = archive(&[
        ("one", EntryType::Regular, b"1234"),
        ("two", EntryType::Regular, b"5678"),
    ])?;
    let entry_limited = ArchiveLimits {
        max_entries: 1,
        ..ArchiveLimits::conservative()
    };
    let size_limited = ArchiveLimits {
        max_file_bytes: 3,
        ..ArchiveLimits::conservative()
    };
    let expanded_limited = ArchiveLimits {
        max_expanded_bytes: 512,
        ..ArchiveLimits::conservative()
    };
    assert!(matches!(
        inspect_tar_zstd(archive.as_slice(), entry_limited),
        Err(ArchiveError::Invalid)
    ));
    assert!(matches!(
        inspect_tar_zstd(archive.as_slice(), size_limited),
        Err(ArchiveError::Invalid)
    ));
    assert!(matches!(
        inspect_tar_zstd(archive.as_slice(), expanded_limited),
        Err(ArchiveError::Invalid)
    ));
    Ok(())
}

#[test]
fn inspection_rejects_invalid_compressed_input() {
    assert!(matches!(
        inspect_tar_zstd(
            b"not a zstd stream".as_slice(),
            ArchiveLimits::conservative()
        ),
        Err(ArchiveError::Invalid)
    ));
}

#[test]
fn inspection_rejects_concatenated_expanded_data_after_tar_end()
-> Result<(), Box<dyn std::error::Error>> {
    let mut payload = archive(&[("srv/app/config", EntryType::Regular, b"safe")])?;
    payload.extend(zstd::stream::encode_all(
        vec![0_u8; 1_048_576].as_slice(),
        0,
    )?);
    let limits = ArchiveLimits {
        max_expanded_bytes: 4_096,
        ..ArchiveLimits::conservative()
    };
    assert!(matches!(
        inspect_tar_zstd(payload.as_slice(), limits),
        Err(ArchiveError::Invalid)
    ));
    Ok(())
}

fn archive(entries: &[(&str, EntryType, &[u8])]) -> Result<Vec<u8>, io::Error> {
    let mut tar = Vec::new();
    for (path, kind, bytes) in entries {
        let mut header = [0_u8; 512];
        write_bytes(&mut header[0..100], path.as_bytes())?;
        write_octal(&mut header[100..108], 0o644)?;
        write_octal(&mut header[108..116], 0)?;
        write_octal(&mut header[116..124], 0)?;
        write_octal(
            &mut header[124..136],
            u64::try_from(bytes.len()).map_err(|_| io::Error::other("entry too large"))?,
        )?;
        write_octal(&mut header[136..148], 0)?;
        header[148..156].fill(b' ');
        header[156] = entry_type_byte(*kind)?;
        if kind.is_symlink() {
            write_bytes(&mut header[157..257], b"target")?;
        }
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        let checksum = header.iter().map(|byte| u64::from(*byte)).sum();
        write_checksum(&mut header[148..156], checksum)?;
        tar.extend_from_slice(&header);
        tar.extend_from_slice(bytes);
        tar.resize(tar.len().next_multiple_of(512), 0);
    }
    tar.extend_from_slice(&[0_u8; 1024]);
    zstd::stream::encode_all(tar.as_slice(), 0)
}

fn entry_type_byte(kind: EntryType) -> Result<u8, io::Error> {
    match kind {
        kind if kind.is_file() => Ok(b'0'),
        kind if kind.is_dir() => Ok(b'5'),
        kind if kind.is_symlink() => Ok(b'2'),
        kind if kind.is_hard_link() => Ok(b'1'),
        _ => Err(io::Error::other("unsupported test entry type")),
    }
}

fn write_bytes(destination: &mut [u8], value: &[u8]) -> Result<(), io::Error> {
    if value.len() >= destination.len() {
        return Err(io::Error::other("tar field is too small"));
    }
    destination[..value.len()].copy_from_slice(value);
    Ok(())
}

fn write_octal(destination: &mut [u8], value: u64) -> Result<(), io::Error> {
    let encoded = format!("{:0width$o}", value, width = destination.len() - 1);
    if encoded.len() >= destination.len() {
        return Err(io::Error::other("tar number field is too small"));
    }
    destination.fill(0);
    destination[..encoded.len()].copy_from_slice(encoded.as_bytes());
    Ok(())
}

fn write_checksum(destination: &mut [u8], value: u64) -> Result<(), io::Error> {
    let encoded = format!("{:06o}", value);
    if encoded.len() != 6 || destination.len() != 8 {
        return Err(io::Error::other("invalid tar checksum"));
    }
    destination[..6].copy_from_slice(encoded.as_bytes());
    destination[6] = 0;
    destination[7] = b' ';
    Ok(())
}
