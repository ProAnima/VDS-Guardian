use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupState {
    Planned,
    Staging,
    Captured,
    Verifying,
    Sealed,
    Failed,
    Cancelled,
    Quarantined,
}

impl BackupState {
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        use BackupState::{
            Cancelled, Captured, Failed, Planned, Quarantined, Sealed, Staging, Verifying,
        };
        matches!(
            (self, next),
            (Planned, Staging)
                | (Planned, Cancelled)
                | (Staging, Captured | Failed | Cancelled)
                | (Captured, Verifying | Quarantined)
                | (Verifying, Sealed | Failed | Quarantined)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::BackupState;

    #[test]
    fn only_verified_flow_can_reach_sealed() {
        let states = [
            BackupState::Planned,
            BackupState::Staging,
            BackupState::Captured,
            BackupState::Verifying,
            BackupState::Sealed,
            BackupState::Failed,
            BackupState::Cancelled,
            BackupState::Quarantined,
        ];
        for state in states {
            assert_eq!(
                state.can_transition_to(BackupState::Sealed),
                state == BackupState::Verifying
            );
        }
    }
}
