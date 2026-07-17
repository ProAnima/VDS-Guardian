use super::drill_manifest;
use guardian_core::{
    BackupId, ManifestSigner, ManifestVerifier, PayloadPath, RunId, SecretStore, Timestamp,
    VdsProfile,
};
use guardian_local_repository::{LocalRepository, SealedBackup};
use std::{error::Error, io};

/// Produces an otherwise valid sealed payload whose tar path would escape the
/// restore root. The drill must therefore reject it during archive inspection.
pub fn create_hostile_archive_backup<S>(
    repository: &LocalRepository,
    credentials: &dyn SecretStore,
    signer: &S,
    profile: &VdsProfile,
) -> Result<SealedBackup, Box<dyn Error>>
where
    S: ManifestSigner + ManifestVerifier,
{
    let backup_id = BackupId::parse("drill-hostile-archive")?;
    let run_id = RunId::parse("drill-hostile-archive-run")?;
    let payload_path = PayloadPath::parse("payload/filesystem-000.tar.zst.enc")?;
    let staging = repository.begin_staging(run_id.clone())?;
    staging.write_payload(
        "filesystem",
        payload_path.clone(),
        "application/zstd",
        &hostile_tar_zstd()?,
    )?;
    let entry = staging.encrypt_and_register_payload_file(
        "filesystem",
        payload_path,
        "application/zstd",
        &backup_id,
        credentials,
    )?;
    let mut manifest = drill_manifest(backup_id.as_str(), run_id, profile)?;
    manifest.add_payload(entry)?;
    staging
        .seal(manifest, Timestamp::parse("2026-07-15T12:00:03Z")?, signer)
        .map_err(Into::into)
}

fn hostile_tar_zstd() -> Result<Vec<u8>, Box<dyn Error>> {
    let contents = b"must not escape the restore root";
    let mut header = [0_u8; 512];
    write_bytes(&mut header[0..100], b"../guardian-escape")?;
    write_octal(&mut header[100..108], 0o644)?;
    write_octal(&mut header[124..136], u64::try_from(contents.len())?)?;
    header[148..156].fill(b' ');
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum = header.iter().map(|byte| u64::from(*byte)).sum();
    write_checksum(&mut header[148..156], checksum)?;
    let mut archive = header.to_vec();
    archive.extend_from_slice(contents);
    archive.resize(archive.len().next_multiple_of(512), 0);
    archive.extend_from_slice(&[0_u8; 1024]);
    Ok(zstd::stream::encode_all(archive.as_slice(), 0)?)
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
