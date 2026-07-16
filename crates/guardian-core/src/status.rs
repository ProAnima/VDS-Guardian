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
            iteration: "Release 0.1 validation — operator path in progress".to_owned(),
            live_operations_enabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FoundationStatus;

    #[test]
    fn validation_release_exposes_implemented_live_operations() {
        let status = FoundationStatus::current();
        assert!(status.live_operations_enabled);
        assert!(status.iteration.contains("operator path in progress"));
    }
}
