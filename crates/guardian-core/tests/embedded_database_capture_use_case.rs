use guardian_core::{
    ArchiveInspectionPort, ArchiveInspectionPortError, AuditPort, BackupId, BackupStoragePort,
    CaptureAuditCode, CapturePortError, EmbeddedDatabaseCapturePort,
    EmbeddedDatabaseCaptureRequest, EmbeddedDatabaseCaptureUseCase, Manifest, ManifestSigner,
    PayloadEntry, PayloadPath, ProfileId, RunId, SealedBackup, StoragePortError, Timestamp,
};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

#[test]
fn transport_failure_is_audited_and_discarded() -> Result<(), Box<dyn std::error::Error>> {
    let storage = FakeStorage::default();
    let audit = FakeAudit::default();
    let use_case = EmbeddedDatabaseCaptureUseCase {
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
    let use_case = EmbeddedDatabaseCaptureUseCase {
        capture: &SuccessfulCapture,
        storage: &storage,
        inspector: &AcceptingInspector,
        audit: &audit,
    };
    assert_eq!(use_case.execute(&request()?)?.logical_role, "database");
    assert_eq!(
        *storage.events.lock().map_err(|_| "lock")?,
        vec!["begin", "reserve", "register"]
    );
    assert!(audit.codes.lock().map_err(|_| "lock")?.is_empty());
    Ok(())
}

#[test]
fn rejected_snapshot_is_audited_and_discarded_before_registration()
-> Result<(), Box<dyn std::error::Error>> {
    let storage = FakeStorage::default();
    let audit = FakeAudit::default();
    let use_case = EmbeddedDatabaseCaptureUseCase {
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
        vec![CaptureAuditCode::DatabasePolicy]
    );
    Ok(())
}

#[test]
fn invalid_database_path_is_rejected_before_any_storage_call()
-> Result<(), Box<dyn std::error::Error>> {
    let storage = FakeStorage::default();
    let audit = FakeAudit::default();
    let use_case = EmbeddedDatabaseCaptureUseCase {
        capture: &SuccessfulCapture,
        storage: &storage,
        inspector: &AcceptingInspector,
        audit: &audit,
    };
    let mut invalid = request()?;
    invalid.database_path = "relative/app.sqlite".to_owned();
    assert!(use_case.execute(&invalid).is_err());
    assert!(storage.events.lock().map_err(|_| "lock")?.is_empty());
    Ok(())
}

fn request() -> Result<EmbeddedDatabaseCaptureRequest, Box<dyn std::error::Error>> {
    Ok(EmbeddedDatabaseCaptureRequest {
        run_id: RunId::parse("run-001")?,
        profile_id: ProfileId::parse("profile-001")?,
        database_path: "/srv/app/app.sqlite".to_owned(),
        payload_path: PayloadPath::parse("payload/database.sqlite.zst")?,
    })
}

struct FailingCapture;
impl EmbeddedDatabaseCapturePort for FailingCapture {
    fn capture_to(
        &self,
        _: &EmbeddedDatabaseCaptureRequest,
        _: &Path,
    ) -> Result<(), CapturePortError> {
        Err(CapturePortError::Transport)
    }
}
struct SuccessfulCapture;
impl EmbeddedDatabaseCapturePort for SuccessfulCapture {
    fn capture_to(
        &self,
        _: &EmbeddedDatabaseCaptureRequest,
        _: &Path,
    ) -> Result<(), CapturePortError> {
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
        Ok(std::env::temp_dir().join("guardian-core-test.sqlite.zst"))
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
            "database",
            path,
            1,
            "0000000000000000000000000000000000000000000000000000000000000000",
            "application/vnd.sqlite3+zstd",
        )
        .map_err(|_| StoragePortError::Rejected)
    }
    fn seal(
        &self,
        _: Manifest,
        _: Timestamp,
        _: &dyn ManifestSigner,
    ) -> Result<SealedBackup, StoragePortError> {
        self.events
            .lock()
            .map_err(|_| StoragePortError::Unavailable)?
            .push("seal");
        Ok(SealedBackup {
            backup_id: BackupId::parse("backup-001").map_err(|_| StoragePortError::Rejected)?,
        })
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
