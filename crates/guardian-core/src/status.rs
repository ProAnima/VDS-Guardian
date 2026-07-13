use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundationStatus {
    pub product: String,
    pub version: String,
    pub iteration: String,
    pub live_operations_enabled: bool,
}

impl FoundationStatus {
    #[must_use]
    pub fn current() -> Self {
        Self {
            product: "VDS Guardian".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            iteration: "Milestone 1 — local repository foundation".to_owned(),
            live_operations_enabled: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FoundationStatus;

    #[test]
    fn milestone_one_still_disables_live_operations() {
        let status = FoundationStatus::current();
        assert!(!status.live_operations_enabled);
    }
}
