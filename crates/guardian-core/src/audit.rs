use crate::RunId;

pub trait AuditPort: Send + Sync {
    fn capture_failed(&self, run_id: &RunId, code: CaptureAuditCode);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureAuditCode {
    Transport,
    ArchivePolicy,
    DatabasePolicy,
    Storage,
}
