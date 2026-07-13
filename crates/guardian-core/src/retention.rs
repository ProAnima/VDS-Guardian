use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionPolicy {
    max_backups: usize,
    minimum_backups: usize,
}

impl RetentionPolicy {
    pub fn new(max_backups: usize, minimum_backups: usize) -> Result<Self, RetentionPolicyError> {
        if max_backups == 0 {
            return Err(RetentionPolicyError::EmptyRepositoryAllowed);
        }
        if minimum_backups > max_backups {
            return Err(RetentionPolicyError::MinimumExceedsMaximum);
        }
        Ok(Self {
            max_backups,
            minimum_backups,
        })
    }

    #[must_use]
    pub fn max_backups(self) -> usize {
        self.max_backups
    }

    #[must_use]
    pub fn minimum_backups(self) -> usize {
        self.minimum_backups
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum RetentionPolicyError {
    #[error("retention must preserve at least one backup")]
    EmptyRepositoryAllowed,
    #[error("retention minimum cannot exceed its maximum")]
    MinimumExceedsMaximum,
}

#[cfg(test)]
mod tests {
    use super::{RetentionPolicy, RetentionPolicyError};

    #[test]
    fn policy_rejects_destructive_bounds() {
        assert_eq!(
            RetentionPolicy::new(0, 0),
            Err(RetentionPolicyError::EmptyRepositoryAllowed)
        );
        assert_eq!(
            RetentionPolicy::new(2, 3),
            Err(RetentionPolicyError::MinimumExceedsMaximum)
        );
        assert!(RetentionPolicy::new(3, 2).is_ok());
    }
}
