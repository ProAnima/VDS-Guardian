//! The argument types of every MCP tool. Field doc comments become the tool's JSON schema
//! descriptions, so they are written for the calling agent.

use rmcp::schemars;
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ProfileIdParams {
    /// The enrolled SSH profile's id.
    pub profile_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BrowseDirectoryParams {
    /// The enrolled SSH profile's id.
    pub profile_id: String,
    /// A validated absolute POSIX directory, for example `/srv`.
    pub directory: String,
    /// Opaque cursor returned by the preceding page, if any.
    pub cursor: Option<String>,
    /// Number of entries requested, from 1 through 200.
    pub limit: u16,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RepositoryIdParams {
    /// The registered repository's id.
    pub repository_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PlanIdParams {
    /// The saved capture plan's id.
    pub plan_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RunCaptureParams {
    /// The saved capture plan's id.
    pub plan_id: String,
    /// A fresh, caller-minted run id (used for cancellation and the audit
    /// trail). Must not be reused across concurrent or prior runs.
    pub run_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSelectionParams {
    /// The enrolled SSH profile that owns the selected data.
    pub profile_id: String,
    /// The ready local repository that will receive the encrypted backup.
    pub repository_id: String,
    /// Filesystem paths and Docker persistent mounts selected from the read-only browser.
    pub items: Vec<CaptureSelectionItemParams>,
    /// Optional SQLite database path for a consistent snapshot payload.
    pub sqlite_path: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum CaptureSelectionItemParams {
    RemotePath {
        absolute_path: String,
    },
    DockerMount {
        container_id: String,
        mount_destination: String,
        capturable_path: String,
    },
    DockerGroup {
        group_id: String,
        capturable_paths: Vec<String>,
    },
}

impl CaptureSelectionParams {
    /// Validates every path and id; any invalid part rejects the whole selection.
    pub(crate) fn parse(self) -> Result<guardian_core::BackupSelection, ()> {
        use guardian_core::{ProfileId, RemotePath, RepositoryId};
        let items = self
            .items
            .into_iter()
            .map(CaptureSelectionItemParams::parse)
            .collect::<Result<_, ()>>()?;
        Ok(guardian_core::BackupSelection {
            profile_id: ProfileId::parse(self.profile_id).map_err(|_| ())?,
            repository_id: RepositoryId::parse(self.repository_id).map_err(|_| ())?,
            items,
            sqlite_path: self
                .sqlite_path
                .map(RemotePath::parse)
                .transpose()
                .map_err(|_| ())?,
        })
    }
}

impl CaptureSelectionItemParams {
    fn parse(self) -> Result<guardian_core::BackupSelectionItem, ()> {
        use guardian_core::{BackupSelectionItem, RemotePath};
        Ok(match self {
            Self::RemotePath { absolute_path } => BackupSelectionItem::RemotePath {
                absolute_path: RemotePath::parse(absolute_path).map_err(|_| ())?,
            },
            Self::DockerMount {
                container_id,
                mount_destination,
                capturable_path,
            } => BackupSelectionItem::DockerMount {
                container_id,
                mount_destination: RemotePath::parse(mount_destination).map_err(|_| ())?,
                capturable_path: RemotePath::parse(capturable_path).map_err(|_| ())?,
            },
            Self::DockerGroup {
                group_id,
                capturable_paths,
            } => BackupSelectionItem::DockerGroup {
                group_id,
                capturable_paths: capturable_paths
                    .into_iter()
                    .map(RemotePath::parse)
                    .collect::<Result<_, _>>()
                    .map_err(|_| ())?,
            },
        })
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PreviewCaptureSelectionParams {
    #[serde(flatten)]
    pub selection: CaptureSelectionParams,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ExecuteCaptureSelectionParams {
    #[serde(flatten)]
    pub selection: CaptureSelectionParams,
    /// The exact confirmation returned by preview_capture_selection for these inputs.
    pub confirmation: String,
    /// A fresh caller-minted run id; use it with cancel_job.
    pub run_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PreviewRestoreParams {
    pub repository_id: String,
    pub backup_id: String,
    /// An absolute local path that does not already exist.
    pub destination: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ExecuteRestoreParams {
    pub repository_id: String,
    pub backup_id: String,
    pub destination: String,
    /// The exact `confirmation` string returned by a prior `preview_restore`
    /// call for these same inputs. Never auto-fill this from another
    /// source — it must come from an explicit preview step, standing in for
    /// the human who would otherwise type or paste it.
    pub confirmation: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PreviewDeployParams {
    pub repository_id: String,
    pub backup_id: String,
    pub target_profile_id: String,
    /// An absolute POSIX path on the remote target host that does not
    /// already exist.
    pub target_path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ExecuteDeployParams {
    pub repository_id: String,
    pub backup_id: String,
    pub target_profile_id: String,
    pub target_path: String,
    /// The exact `confirmation` string returned by a prior `preview_deploy`
    /// call for these same inputs.
    pub confirmation: String,
    /// A fresh, caller-minted run id (used for cancellation and the audit
    /// trail). Must not be reused across concurrent or prior runs.
    pub run_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CancelJobParams {
    /// The `run_id` originally passed to `run_capture` or `execute_deploy`.
    pub run_id: String,
}
