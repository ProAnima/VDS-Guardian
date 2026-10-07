//! MCP (Model Context Protocol) server exposing VDS Guardian's
//! capture/restore/deploy operations to external tools and AI agents.
//!
//! Stdio transport only: a stdio pipe is only reachable by the direct
//! parent/child process relationship, so this server inherits the OS
//! process trust boundary exactly like `guardian-cli` and the desktop app
//! already do — it does not create a new, wider trust boundary. See ADR
//! 0012 for the full design and its rejected alternatives (streamable HTTP,
//! folding into `guardian-cli`, auto-supplied confirmation).
//!
//! Every tool handler here stays as thin as `guardian-cli`'s own command
//! functions: validate arguments, call one function in the matching domain
//! module (`capture`, `restore`, `deploy`, `discovery`), map one typed
//! result. The domain modules hold the actual logic and are unit-tested
//! directly; this file only wires them to the MCP protocol.

mod capture;
mod config;
mod deploy;
mod discovery;
mod params;
mod response;
mod restore;
mod secret_store;

pub use params::*;

use config::ServerConfig;
use guardian_core::JobRegistry;
use response::{err, invalid_selection, ok, respond};
use rmcp::{
    ErrorData, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::stdio,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct GuardianMcpServer {
    config: Arc<ServerConfig>,
    jobs: Arc<JobRegistry>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl GuardianMcpServer {
    #[must_use]
    pub fn new(config: ServerConfig) -> Self {
        Self {
            config: Arc::new(config),
            jobs: Arc::new(JobRegistry::default()),
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "List already-enrolled SSH server profiles.")]
    async fn list_ssh_profiles(&self) -> Result<rmcp::model::CallToolResult, ErrorData> {
        respond(discovery::list_ssh_profiles(&self.config))
    }

    #[tool(description = "List registered local/removable backup repositories.")]
    async fn list_repositories(&self) -> Result<rmcp::model::CallToolResult, ErrorData> {
        respond(discovery::list_repositories(&self.config))
    }

    #[tool(description = "List saved capture plans (profile + repository + roots).")]
    async fn list_capture_plans(&self) -> Result<rmcp::model::CallToolResult, ErrorData> {
        respond(discovery::list_capture_plans(&self.config))
    }

    #[tool(
        description = "List Docker containers and their capturable mounts on an enrolled server."
    )]
    async fn list_docker_containers(
        &self,
        Parameters(params): Parameters<ProfileIdParams>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        respond(discovery::list_docker_containers(
            &self.config,
            &params.profile_id,
        ))
    }

    #[tool(
        description = "Read one bounded page from a directory on an enrolled server. Read-only, pinned SSH; symlinks are never followed or selectable."
    )]
    async fn browse_remote_directory(
        &self,
        Parameters(params): Parameters<BrowseDirectoryParams>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        respond(discovery::browse_remote_directory(
            &self.config,
            &params.profile_id,
            &params.directory,
            params.cursor,
            params.limit,
        ))
    }

    #[tool(description = "List a repository's sealed, verified backups.")]
    async fn list_backups(
        &self,
        Parameters(params): Parameters<RepositoryIdParams>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        respond(discovery::list_backups(&self.config, &params.repository_id))
    }

    #[tool(
        description = "Preview a saved capture plan (its profile and repository) without running it."
    )]
    async fn plan_capture(
        &self,
        Parameters(params): Parameters<PlanIdParams>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        respond(capture::plan_capture(&self.config, &params.plan_id))
    }

    #[tool(
        description = "Run a saved capture plan, sealing a new verified, encrypted backup. No confirmation phrase exists for capture (matching every other surface); cancellable via cancel_job using the same run_id."
    )]
    async fn run_capture(
        &self,
        Parameters(params): Parameters<RunCaptureParams>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        respond(capture::run_capture(
            &self.config,
            &self.jobs,
            &params.plan_id,
            &params.run_id,
        ))
    }

    #[tool(
        description = "Preview a browser-derived filesystem/Docker backup selection. Read-only; returns the exact confirmation required by execute_capture_selection."
    )]
    async fn preview_capture_selection(
        &self,
        Parameters(params): Parameters<PreviewCaptureSelectionParams>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        let Ok(selection) = params.selection.parse() else {
            return invalid_selection();
        };
        respond(capture::preview_selection(&self.config, &selection))
    }

    #[tool(
        description = "Create a verified encrypted backup from a filesystem/Docker selection. Re-resolves Docker data and requires the exact confirmation from preview_capture_selection; cancellable via cancel_job."
    )]
    async fn execute_capture_selection(
        &self,
        Parameters(params): Parameters<ExecuteCaptureSelectionParams>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        let Ok(selection) = params.selection.parse() else {
            return invalid_selection();
        };
        respond(capture::execute_selection(
            &self.config,
            &self.jobs,
            &selection,
            &params.confirmation,
            &params.run_id,
        ))
    }

    #[tool(
        description = "Preview restoring a sealed backup to a new local destination. Returns a confirmation phrase required by execute_restore."
    )]
    async fn preview_restore(
        &self,
        Parameters(params): Parameters<PreviewRestoreParams>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        respond(restore::preview_restore(
            &self.config,
            &params.repository_id,
            &params.backup_id,
            &params.destination,
        ))
    }

    #[tool(
        description = "Restore a sealed backup to a new local destination. Requires the exact confirmation phrase from a prior preview_restore call. Not cancellable (local disk copy, no SSH child)."
    )]
    async fn execute_restore(
        &self,
        Parameters(params): Parameters<ExecuteRestoreParams>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        respond(restore::execute_restore(
            &self.config,
            &params.repository_id,
            &params.backup_id,
            &params.destination,
            &params.confirmation,
        ))
    }

    #[tool(
        description = "Preview deploying a sealed backup to a different, already-enrolled target server. Returns a confirmation phrase required by execute_deploy."
    )]
    async fn preview_deploy(
        &self,
        Parameters(params): Parameters<PreviewDeployParams>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        respond(deploy::preview_deploy(
            &self.config,
            &params.repository_id,
            &params.backup_id,
            &params.target_profile_id,
            &params.target_path,
        ))
    }

    #[tool(
        description = "Deploy a sealed backup to a different, already-enrolled target server. Requires the exact confirmation phrase from a prior preview_deploy call. Cancellable via cancel_job using the same run_id."
    )]
    async fn execute_deploy(
        &self,
        Parameters(params): Parameters<ExecuteDeployParams>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        respond(deploy::execute_deploy(
            &self.config,
            &self.jobs,
            &params.repository_id,
            &params.backup_id,
            &params.target_profile_id,
            &params.target_path,
            &params.confirmation,
            &params.run_id,
        ))
    }

    #[tool(
        description = "Request cancellation of a running run_capture or execute_deploy job by its run_id. Cooperative: the job stops at its next poll tick, not instantly."
    )]
    async fn cancel_job(
        &self,
        Parameters(params): Parameters<CancelJobParams>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        let Ok(run_id) = guardian_core::RunId::parse(&params.run_id) else {
            return Ok(err(&serde_json::json!({
                "code": "invalid_run_id",
                "message": "The run id is not a valid identifier.",
            })));
        };
        let cancelled = self.jobs.cancel(&run_id);
        Ok(ok(&serde_json::json!({ "cancelled": cancelled })))
    }
}

#[tool_handler]
impl ServerHandler for GuardianMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "VDS Guardian backup/restore/deploy tools. The desktop app is the \
                 first-class human interface; this server exists for headless and \
                 agent-driven access over the same operations. execute_restore and \
                 execute_deploy require the exact confirmation phrase from a prior \
                 preview call — never guess or reuse one from a different backup, \
                 destination, or target."
                    .to_owned(),
            ),
            ..Default::default()
        }
    }
}

pub fn run(arguments: &[std::ffi::OsString]) -> Result<(), Box<dyn std::error::Error>> {
    let config = ServerConfig::parse(arguments).map_err(|_| "invalid startup arguments")?;
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let service = GuardianMcpServer::new(config).serve(stdio()).await?;
        service.waiting().await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })
}

#[cfg(test)]
mod tests;
