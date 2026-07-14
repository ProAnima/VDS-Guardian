use guardian_core::{FilesystemCaptureRequest, PayloadPath, ProfileId, RunId};

#[test]
fn capture_request_rejects_unreviewed_remote_roots() -> Result<(), Box<dyn std::error::Error>> {
    let request = FilesystemCaptureRequest {
        run_id: RunId::parse("run-001")?,
        profile_id: ProfileId::parse("profile-001")?,
        roots: vec!["/srv/app".to_owned()],
        payload_path: PayloadPath::parse("payload/filesystem.tar.zst")?,
    };
    request.validate()?;
    let unsafe_request = FilesystemCaptureRequest {
        roots: vec!["/srv/../etc".to_owned()],
        ..request
    };
    assert!(unsafe_request.validate().is_err());
    Ok(())
}
