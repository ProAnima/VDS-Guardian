use guardian_archive::TarZstdWriter;
use guardian_capture::{CaptureTransportError, FilesystemCaptureTransport, capture_filesystem};
use guardian_core::{ArchivePath, PayloadPath, RepositoryId, RunId};
use guardian_local_repository::LocalRepository;
use std::{fs, path::Path};

#[test]
fn pipeline_inspects_before_registering_a_staged_payload() -> Result<(), Box<dyn std::error::Error>>
{
    let temporary = tempfile::tempdir()?;
    let repository = LocalRepository::open(temporary.path(), RepositoryId::parse("repo-capture")?)?;
    let staging = repository.begin_staging(RunId::parse("run-capture")?)?;
    let captured = capture_filesystem(
        &staging,
        &FixtureTransport {
            bytes: valid_archive()?,
        },
        "filesystem",
        PayloadPath::parse("payload/filesystem-000.tar.zst")?,
        guardian_archive::ArchiveLimits::conservative(),
    )?;
    assert_eq!(captured.inspection.regular_files, 1);
    assert_eq!(captured.payload.logical_role, "filesystem");
    Ok(())
}

#[test]
fn invalid_archive_is_removed_and_never_registered() -> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let repository = LocalRepository::open(temporary.path(), RepositoryId::parse("repo-reject")?)?;
    let staging = repository.begin_staging(RunId::parse("run-reject")?)?;
    let path = PayloadPath::parse("payload/filesystem-000.tar.zst")?;
    assert!(
        capture_filesystem(
            &staging,
            &FixtureTransport {
                bytes: b"bad".to_vec()
            },
            "filesystem",
            path.clone(),
            guardian_archive::ArchiveLimits::conservative()
        )
        .is_err()
    );
    assert!(!repository.root().join(path.as_str()).exists());
    Ok(())
}

struct FixtureTransport {
    bytes: Vec<u8>,
}
impl FilesystemCaptureTransport for FixtureTransport {
    fn capture_to(&self, destination: &Path) -> Result<(), CaptureTransportError> {
        fs::write(destination, &self.bytes).map_err(|_| CaptureTransportError::Failed)
    }
}

fn valid_archive() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut writer = TarZstdWriter::new(Vec::new())?;
    let path = ArchivePath::parse("srv/app/config.yaml")?;
    writer.append_file(&path, 5, &mut "safe\n".as_bytes())?;
    Ok(writer.finish()?)
}
