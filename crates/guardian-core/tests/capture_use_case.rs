use guardian_core::{
    AuditPort, BackupStoragePort, CaptureAuditCode, CapturePortError, CapturedStream,
    FilesystemCapturePort, FilesystemCaptureRequest, FilesystemCaptureUseCase, PayloadEntry,
    PayloadPath, ProfileId, RunId, StoragePortError,
};
use std::sync::Mutex;

#[test]
fn transport_failure_is_audited_and_discarded() -> Result<(), Box<dyn std::error::Error>> {
    let storage = FakeStorage::default();
    let audit = FakeAudit::default();
    let use_case = FilesystemCaptureUseCase {
        capture: &FailingCapture,
        storage: &storage,
        audit: &audit,
    };
    assert!(use_case.execute(&request()?).is_err());
    assert_eq!(
        *storage.events.lock().map_err(|_| "lock")?,
        vec!["begin", "discard"]
    );
    assert_eq!(
        *audit.codes.lock().map_err(|_| "lock")?,
        vec![CaptureAuditCode::Transport]
    );
    Ok(())
}

#[test]
fn successful_capture_registers_the_payload() -> Result<(), Box<dyn std::error::Error>> {
    let storage = FakeStorage::default();
    let audit = FakeAudit::default();
    let use_case = FilesystemCaptureUseCase {
        capture: &SuccessfulCapture,
        storage: &storage,
        audit: &audit,
    };
    assert_eq!(use_case.execute(&request()?)?.logical_role, "filesystem");
    assert_eq!(
        *storage.events.lock().map_err(|_| "lock")?,
        vec!["begin", "register"]
    );
    assert!(audit.codes.lock().map_err(|_| "lock")?.is_empty());
    Ok(())
}

fn request() -> Result<FilesystemCaptureRequest, Box<dyn std::error::Error>> {
    Ok(FilesystemCaptureRequest {
        run_id: RunId::parse("run-001")?,
        profile_id: ProfileId::parse("profile-001")?,
        roots: vec!["/srv/app".to_owned()],
        payload_path: PayloadPath::parse("payload/filesystem.tar.zst")?,
    })
}

struct FailingCapture;
impl FilesystemCapturePort for FailingCapture {
    fn capture(&self, _: &FilesystemCaptureRequest) -> Result<CapturedStream, CapturePortError> {
        Err(CapturePortError::Transport)
    }
}
struct SuccessfulCapture;
impl FilesystemCapturePort for SuccessfulCapture {
    fn capture(&self, _: &FilesystemCaptureRequest) -> Result<CapturedStream, CapturePortError> {
        Ok(CapturedStream {
            payload: PayloadEntry::new(
                "filesystem",
                PayloadPath::parse("payload/filesystem.tar.zst")
                    .map_err(|_| CapturePortError::Transport)?,
                1,
                "0000000000000000000000000000000000000000000000000000000000000000",
                "application/zstd",
            )
            .map_err(|_| CapturePortError::Transport)?,
        })
    }
}

#[derive(Default)]
struct FakeStorage {
    events: Mutex<Vec<&'static str>>,
}
impl BackupStoragePort for FakeStorage {
    fn begin(&self, _: &RunId) -> Result<(), StoragePortError> {
        self.events
            .lock()
            .map_err(|_| StoragePortError::Unavailable)?
            .push("begin");
        Ok(())
    }
    fn register_payload(&self, _: PayloadEntry) -> Result<(), StoragePortError> {
        self.events
            .lock()
            .map_err(|_| StoragePortError::Unavailable)?
            .push("register");
        Ok(())
    }
    fn discard(&self, _: &RunId) -> Result<(), StoragePortError> {
        self.events
            .lock()
            .map_err(|_| StoragePortError::Unavailable)?
            .push("discard");
        Ok(())
    }
}
#[derive(Default)]
struct FakeAudit {
    codes: Mutex<Vec<CaptureAuditCode>>,
}
impl AuditPort for FakeAudit {
    fn capture_failed(&self, _: &RunId, code: CaptureAuditCode) {
        if let Ok(mut codes) = self.codes.lock() {
            codes.push(code);
        }
    }
}
