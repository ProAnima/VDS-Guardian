use guardian_core::{
    ArchiveInspectionPort, ArchiveInspectionPortError, AuditPort, BackupStoragePort,
    CaptureAuditCode, CapturePortError, FilesystemCapturePort, FilesystemCaptureRequest,
    FilesystemCaptureUseCase, PayloadEntry, PayloadPath, ProfileId, RunId, StoragePortError,
};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

#[test]
fn transport_failure_is_audited_and_discarded() -> Result<(), Box<dyn std::error::Error>> {
    let storage = FakeStorage::default();
    let audit = FakeAudit::default();
    let use_case = FilesystemCaptureUseCase {
        capture: &FailingCapture,
        storage: &storage,
        inspector: &AcceptingInspector,
        audit: &audit,
    };
    assert!(use_case.execute(&request()?).is_err());
    assert_eq!(
        *storage.events.lock().map_err(|_| "lock")?,
        vec!["begin", "reserve", "discard"]
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
        inspector: &AcceptingInspector,
        audit: &audit,
    };
    assert_eq!(use_case.execute(&request()?)?.logical_role, "filesystem");
    assert_eq!(
        *storage.events.lock().map_err(|_| "lock")?,
        vec!["begin", "reserve", "register"]
    );
    assert!(audit.codes.lock().map_err(|_| "lock")?.is_empty());
    Ok(())
}

#[test]
fn rejected_archive_is_audited_and_discarded_before_registration()
-> Result<(), Box<dyn std::error::Error>> {
    let storage = FakeStorage::default();
    let audit = FakeAudit::default();
    let use_case = FilesystemCaptureUseCase {
        capture: &SuccessfulCapture,
        storage: &storage,
        inspector: &RejectingInspector,
        audit: &audit,
    };
    assert!(use_case.execute(&request()?).is_err());
    assert_eq!(
        *storage.events.lock().map_err(|_| "lock")?,
        vec!["begin", "reserve", "discard"]
    );
    assert_eq!(
        *audit.codes.lock().map_err(|_| "lock")?,
        vec![CaptureAuditCode::ArchivePolicy]
    );
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
    fn capture_to(&self, _: &FilesystemCaptureRequest, _: &Path) -> Result<(), CapturePortError> {
        Err(CapturePortError::Transport)
    }
}
struct SuccessfulCapture;
impl FilesystemCapturePort for SuccessfulCapture {
    fn capture_to(&self, _: &FilesystemCaptureRequest, _: &Path) -> Result<(), CapturePortError> {
        Ok(())
    }
}
struct AcceptingInspector;
impl ArchiveInspectionPort for AcceptingInspector {
    fn inspect(&self, _: &Path) -> Result<(), ArchiveInspectionPortError> {
        Ok(())
    }
}
struct RejectingInspector;
impl ArchiveInspectionPort for RejectingInspector {
    fn inspect(&self, _: &Path) -> Result<(), ArchiveInspectionPortError> {
        Err(ArchiveInspectionPortError::Rejected)
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
    fn reserve(&self, _: &PayloadPath) -> Result<PathBuf, StoragePortError> {
        self.events
            .lock()
            .map_err(|_| StoragePortError::Unavailable)?
            .push("reserve");
        Ok(std::env::temp_dir().join("guardian-core-test.tar.zst"))
    }
    fn register_payload_path(
        &self,
        _: &str,
        path: PayloadPath,
        _: &str,
    ) -> Result<PayloadEntry, StoragePortError> {
        self.events
            .lock()
            .map_err(|_| StoragePortError::Unavailable)?
            .push("register");
        PayloadEntry::new(
            "filesystem",
            path,
            1,
            "0000000000000000000000000000000000000000000000000000000000000000",
            "application/zstd",
        )
        .map_err(|_| StoragePortError::Rejected)
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
