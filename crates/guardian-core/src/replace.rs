use crate::manifest::{PayloadSelectionError, select_payloads};
use crate::{
    BackupId, DockerContainer, DockerContainerState, DockerInventory, DockerMountSnapshot,
    DockerWorkloadSnapshot, Manifest, ManifestError, PayloadPath, ProfileId, RemotePath,
    VdsProfile,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceReplacementImpact {
    pub backup_id: BackupId,
    pub target_profile_id: ProfileId,
    pub root: RemotePath,
    pub containers: Vec<String>,
    pub replaces: Vec<RemotePath>,
    pub conflicts: Vec<String>,
    pub safety_backup_required: bool,
    pub service_stop_required: bool,
    pub confirmation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceReplacementPlan {
    pub impact: SourceReplacementImpact,
    pub filesystem_payload: PayloadPath,
    docker_workloads: Vec<DockerWorkloadSnapshot>,
}

impl SourceReplacementPlan {
    pub fn build(
        manifest: &Manifest,
        target: &VdsProfile,
    ) -> Result<Self, SourceReplacementPlanError> {
        manifest
            .validate_sealed()
            .map_err(SourceReplacementPlanError::Manifest)?;
        target
            .validate()
            .map_err(|_| SourceReplacementPlanError::TargetMismatch)?;
        if manifest.source.profile_id != target.profile_id
            || manifest.source.host_key_fingerprint
                != crate::host_key_fingerprint(&target.endpoint.host_pin.public_key_base64)
        {
            return Err(SourceReplacementPlanError::TargetMismatch);
        }
        let layout = manifest
            .source_layout
            .as_ref()
            .ok_or(SourceReplacementPlanError::MissingLayout)?;
        if layout.roots.len() != 1 {
            return Err(SourceReplacementPlanError::MultipleRoots);
        }
        let (filesystem_payload, database_payload) = select_payloads(manifest)?;
        if database_payload.is_some() {
            return Err(SourceReplacementPlanError::DatabaseRequiresAdapter);
        }
        let root = layout.roots[0].clone();
        if protected_root(root.as_str()) {
            return Err(SourceReplacementPlanError::UnsafeRoot);
        }
        let containers = layout
            .docker_workloads
            .iter()
            .filter(|workload| was_active(workload.state))
            .map(|workload| workload.container_name.clone())
            .collect::<Vec<_>>();
        let confirmation = format!(
            "REPLACE {} ON {} AT {}",
            manifest.backup_id.as_str(),
            target.profile_id.as_str(),
            root.as_str(),
        );
        Ok(Self {
            impact: SourceReplacementImpact {
                backup_id: manifest.backup_id.clone(),
                target_profile_id: target.profile_id.clone(),
                root: root.clone(),
                containers: containers.clone(),
                replaces: vec![root],
                conflicts: Vec::new(),
                safety_backup_required: true,
                service_stop_required: !containers.is_empty(),
                confirmation,
            },
            filesystem_payload,
            docker_workloads: layout.docker_workloads.clone(),
        })
    }

    #[must_use]
    pub fn reconcile_current(mut self, inventory: Option<&DockerInventory>) -> Self {
        self.reconcile_workloads(inventory);
        self.refresh_confirmation();
        self
    }

    #[must_use]
    pub fn reconcile_source_ready(mut self, ready: bool) -> Self {
        if !ready {
            self.impact.conflicts.push(format!(
                "source_root_unavailable:{}",
                self.impact.root.as_str()
            ));
        }
        self.refresh_confirmation();
        self
    }

    fn reconcile_workloads(&mut self, inventory: Option<&DockerInventory>) {
        self.impact.containers.clear();
        self.impact.conflicts.clear();
        if self.docker_workloads.is_empty() {
            self.impact.service_stop_required = false;
            return;
        }
        let Some(inventory) = inventory else {
            self.impact
                .conflicts
                .push("docker_inventory_unavailable".to_owned());
            return;
        };
        for expected in &self.docker_workloads {
            let current = inventory
                .containers
                .iter()
                .find(|item| item.id == expected.container_id);
            let Some(current) = current else {
                self.impact
                    .conflicts
                    .push(format!("container_missing:{}", expected.container_name));
                continue;
            };
            compare_workload(expected, current, &mut self.impact.conflicts);
            if was_active(current.state) {
                self.impact.containers.push(current.name.clone());
            }
        }
        self.impact.containers.sort();
        self.impact.containers.dedup();
        self.impact.service_stop_required = !self.impact.containers.is_empty();
    }

    fn refresh_confirmation(&mut self) {
        let mut state = Sha256::new();
        state.update(self.impact.root.as_str().as_bytes());
        for container in &self.impact.containers {
            state.update(container.as_bytes());
        }
        for conflict in &self.impact.conflicts {
            state.update(conflict.as_bytes());
        }
        let token = format!("{:x}", state.finalize());
        self.impact.confirmation = format!(
            "REPLACE {} ON {} AT {} STATE {}",
            self.impact.backup_id.as_str(),
            self.impact.target_profile_id.as_str(),
            self.impact.root.as_str(),
            &token[..12],
        );
    }

    pub fn approve(&self, confirmation: &str) -> Result<(), SourceReplacementPlanError> {
        if !self.impact.conflicts.is_empty() {
            return Err(SourceReplacementPlanError::LiveConflicts);
        }
        (confirmation == self.impact.confirmation)
            .then_some(())
            .ok_or(SourceReplacementPlanError::ConfirmationRequired)
    }
}

fn was_active(state: DockerContainerState) -> bool {
    matches!(
        state,
        DockerContainerState::Running
            | DockerContainerState::Paused
            | DockerContainerState::Restarting
    )
}

fn protected_root(value: &str) -> bool {
    matches!(
        value,
        "/" | "/bin"
            | "/boot"
            | "/dev"
            | "/etc"
            | "/lib"
            | "/lib64"
            | "/proc"
            | "/run"
            | "/sbin"
            | "/sys"
            | "/usr"
    )
}

fn compare_workload(
    expected: &DockerWorkloadSnapshot,
    current: &DockerContainer,
    conflicts: &mut Vec<String>,
) {
    if current.name != expected.container_name {
        conflicts.push(format!(
            "container_name_changed:{}",
            expected.container_name
        ));
    }
    if current.image != expected.image || current.image_digest != expected.image_digest {
        conflicts.push(format!(
            "container_image_changed:{}",
            expected.container_name
        ));
    }
    if current.compose_project != expected.compose_project {
        conflicts.push(format!(
            "container_compose_changed:{}",
            expected.container_name
        ));
    }
    if expected
        .mounts
        .iter()
        .any(|mount| !mount_matches(mount, current))
    {
        conflicts.push(format!(
            "container_mount_changed:{}",
            expected.container_name
        ));
    }
}

fn mount_matches(expected: &DockerMountSnapshot, current: &DockerContainer) -> bool {
    current.mounts.iter().any(|mount| {
        mount.destination == expected.destination.as_str()
            && mount.capturable_path() == Some(expected.source_path.as_str())
            && mount.read_only == expected.read_only
    })
}

#[derive(Debug, Error)]
pub enum SourceReplacementPlanError {
    #[error("backup manifest is not a verified sealed backup")]
    Manifest(#[source] ManifestError),
    #[error("backup does not contain signed source layout metadata")]
    MissingLayout,
    #[error("source replacement target does not match the captured server")]
    TargetMismatch,
    #[error("multi-root replacement is not supported by this transaction")]
    MultipleRoots,
    #[error("replacement of this operating-system root is not allowed")]
    UnsafeRoot,
    #[error("database payload requires a database-aware replacement adapter")]
    DatabaseRequiresAdapter,
    #[error("backup has no supported filesystem payload")]
    NoFilesystemPayload,
    #[error("backup has more than one database payload")]
    AmbiguousDatabasePayload,
    #[error("exact source replacement confirmation is required")]
    ConfirmationRequired,
    #[error("current source state has blocking replacement conflicts")]
    LiveConflicts,
}

impl From<PayloadSelectionError> for SourceReplacementPlanError {
    fn from(error: PayloadSelectionError) -> Self {
        match error {
            PayloadSelectionError::NoFilesystemPayload => Self::NoFilesystemPayload,
            PayloadSelectionError::AmbiguousDatabasePayload => Self::AmbiguousDatabasePayload,
        }
    }
}
