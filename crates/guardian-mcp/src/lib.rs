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
mod restore;
mod secret_store;

use config::ServerConfig;
use guardian_core::JobRegistry;
use rmcp::{
    ErrorData, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Serializes a successful domain result into a structured tool response.
/// Falls back to a structured error rather than `unwrap`/`panic` on the
/// (in practice unreachable, since every DTO here is a plain
/// string/option-of-string struct) chance that serialization itself fails.
fn ok<T: Serialize>(value: &T) -> rmcp::model::CallToolResult {
    match serde_json::to_value(value) {
        Ok(json) => rmcp::model::CallToolResult::structured(json),
        Err(_) => rmcp::model::CallToolResult::structured_error(serde_json::json!({
            "code": "serialization_failed",
            "message": "The tool result could not be serialized.",
        })),
    }
}

/// Serializes a domain failure into a structured, tool-level error — the
/// request was valid and reached the right composition, but the operation
/// itself did not succeed (not found, rejected, cancelled, ...). This is
/// deliberately not a protocol-level `ErrorData`: MCP clients render
/// protocol errors opaquely, but the caller (an operator or an agent acting
/// on their behalf) needs to see *why* a restore or deploy was rejected.
fn err<T: Serialize>(value: &T) -> rmcp::model::CallToolResult {
    match serde_json::to_value(value) {
        Ok(json) => rmcp::model::CallToolResult::structured_error(json),
        Err(_) => rmcp::model::CallToolResult::structured_error(serde_json::json!({
            "code": "serialization_failed",
            "message": "The tool failure could not be serialized.",
        })),
    }
}

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
        Ok(match discovery::list_ssh_profiles(&self.config) {
            Ok(profiles) => ok(&profiles),
            Err(failure) => err(&failure),
        })
    }

    #[tool(description = "List registered local/removable backup repositories.")]
    async fn list_repositories(&self) -> Result<rmcp::model::CallToolResult, ErrorData> {
        Ok(match discovery::list_repositories(&self.config) {
            Ok(repositories) => ok(&repositories),
            Err(failure) => err(&failure),
        })
    }

    #[tool(description = "List saved capture plans (profile + repository + roots).")]
    async fn list_capture_plans(&self) -> Result<rmcp::model::CallToolResult, ErrorData> {
        Ok(match discovery::list_capture_plans(&self.config) {
            Ok(plans) => ok(&plans),
            Err(failure) => err(&failure),
        })
    }

    #[tool(
        description = "List Docker containers and their capturable mounts on an enrolled server."
    )]
    async fn list_docker_containers(
        &self,
        Parameters(params): Parameters<ProfileIdParams>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        Ok(
            match discovery::list_docker_containers(&self.config, &params.profile_id) {
                Ok(containers) => ok(&containers),
                Err(failure) => err(&failure),
            },
        )
    }

    #[tool(
        description = "Read one bounded page from a directory on an enrolled server. Read-only, pinned SSH; symlinks are never followed or selectable."
    )]
    async fn browse_remote_directory(
        &self,
        Parameters(params): Parameters<BrowseDirectoryParams>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        Ok(
            match discovery::browse_remote_directory(
                &self.config,
                &params.profile_id,
                &params.directory,
                params.cursor,
                params.limit,
            ) {
                Ok(page) => ok(&page),
                Err(failure) => err(&failure),
            },
        )
    }

    #[tool(description = "List a repository's sealed, verified backups.")]
    async fn list_backups(
        &self,
        Parameters(params): Parameters<RepositoryIdParams>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        Ok(
            match discovery::list_backups(&self.config, &params.repository_id) {
                Ok(backups) => ok(&backups),
                Err(failure) => err(&failure),
            },
        )
    }

    #[tool(
        description = "Preview a saved capture plan (its profile and repository) without running it."
    )]
    async fn plan_capture(
        &self,
        Parameters(params): Parameters<PlanIdParams>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        Ok(match capture::plan_capture(&self.config, &params.plan_id) {
            Ok(preview) => ok(&preview),
            Err(failure) => err(&failure),
        })
    }

    #[tool(
        description = "Run a saved capture plan, sealing a new verified, encrypted backup. No confirmation phrase exists for capture (matching every other surface); cancellable via cancel_job using the same run_id."
    )]
    async fn run_capture(
        &self,
        Parameters(params): Parameters<RunCaptureParams>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        Ok(
            match capture::run_capture(&self.config, &self.jobs, &params.plan_id, &params.run_id) {
                Ok(summary) => ok(&summary),
                Err(failure) => err(&failure),
            },
        )
    }

    #[tool(
        description = "Preview restoring a sealed backup to a new local destination. Returns a confirmation phrase required by execute_restore."
    )]
    async fn preview_restore(
        &self,
        Parameters(params): Parameters<PreviewRestoreParams>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        Ok(
            match restore::preview_restore(
                &self.config,
                &params.repository_id,
                &params.backup_id,
                &params.destination,
            ) {
                Ok(preview) => ok(&preview),
                Err(failure) => err(&failure),
            },
        )
    }

    #[tool(
        description = "Restore a sealed backup to a new local destination. Requires the exact confirmation phrase from a prior preview_restore call. Not cancellable (local disk copy, no SSH child)."
    )]
    async fn execute_restore(
        &self,
        Parameters(params): Parameters<ExecuteRestoreParams>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        Ok(
            match restore::execute_restore(
                &self.config,
                &params.repository_id,
                &params.backup_id,
                &params.destination,
                &params.confirmation,
            ) {
                Ok(preview) => ok(&preview),
                Err(failure) => err(&failure),
            },
        )
    }

    #[tool(
        description = "Preview deploying a sealed backup to a different, already-enrolled target server. Returns a confirmation phrase required by execute_deploy."
    )]
    async fn preview_deploy(
        &self,
        Parameters(params): Parameters<PreviewDeployParams>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        Ok(
            match deploy::preview_deploy(
                &self.config,
                &params.repository_id,
                &params.backup_id,
                &params.target_profile_id,
                &params.target_path,
            ) {
                Ok(preview) => ok(&preview),
                Err(failure) => err(&failure),
            },
        )
    }

    #[tool(
        description = "Deploy a sealed backup to a different, already-enrolled target server. Requires the exact confirmation phrase from a prior preview_deploy call. Cancellable via cancel_job using the same run_id."
    )]
    async fn execute_deploy(
        &self,
        Parameters(params): Parameters<ExecuteDeployParams>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        Ok(
            match deploy::execute_deploy(
                &self.config,
                &self.jobs,
                &params.repository_id,
                &params.backup_id,
                &params.target_profile_id,
                &params.target_path,
                &params.confirmation,
                &params.run_id,
            ) {
                Ok(preview) => ok(&preview),
                Err(failure) => err(&failure),
            },
        )
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
mod tests {
    use super::{GuardianMcpServer, ServerConfig};
    use rmcp::{ClientHandler, ServiceExt};

    #[test]
    fn excluded_tools_stay_excluded() {
        // Real database (`enroll_or_load` etc.) never appears as a tool name,
        // so an enrollment/credential-import/vault-init/signing-enroll/
        // save_capture_plan/recovery-key tool can never be accidentally
        // reintroduced without this test failing. `recovery` alone (ADR
        // 0013) covers init/export/import together — the single
        // highest-blast-radius secret in the system, for the same
        // one-time-bootstrap/secret-bearing reason the others are excluded.
        let forbidden = [
            "enroll",
            "import_ssh_key",
            "register_agent_key",
            "register_repository",
            "vault_init",
            "signing_enroll",
            "save_capture_plan",
            "recovery",
        ];
        let tools = GuardianMcpServer::tool_router().list_all();
        for tool in &tools {
            for banned in forbidden {
                assert!(
                    !tool.name.contains(banned),
                    "tool {:?} must not exist in v1's tool surface",
                    tool.name
                );
            }
        }
    }

    #[derive(Default, Clone)]
    struct TestClient;
    impl ClientHandler for TestClient {}

    /// A real MCP protocol round trip over an in-memory duplex pair (not a
    /// live subprocess/stdio handshake, but a genuine client/server exchange
    /// through the real `rmcp` wire protocol, not just a direct Rust call).
    /// Confirms the server actually speaks MCP: initializes, advertises the
    /// expected tools, and answers a real `tools/call`. A true external-
    /// process stdio round trip (a real Claude Code/Desktop subprocess
    /// launch) is not exercised by any automated test — named honestly
    /// rather than silently skipped, matching this project's established
    /// pattern for live-round-trip gaps (ADR 0009, ADR 0010).
    #[tokio::test]
    async fn serves_real_mcp_requests_over_an_in_memory_transport()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let config = ServerConfig {
            repositories_dir: root.path().join("repositories"),
            profiles_dir: root.path().join("profiles"),
            plans_dir: root.path().join("plans"),
            config_dir: root.path().join("node"),
            vault_dir: None,
        };
        let (server_transport, client_transport) = tokio::io::duplex(4096);
        let server = GuardianMcpServer::new(config);
        let server_handle = tokio::spawn(async move {
            let service = server.serve(server_transport).await?;
            service.waiting().await?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        });

        let client = TestClient.serve(client_transport).await?;
        let tools = client.list_all_tools().await?;
        let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
        for expected in [
            "list_ssh_profiles",
            "browse_remote_directory",
            "run_capture",
            "preview_restore",
            "execute_deploy",
            "cancel_job",
        ] {
            assert!(names.contains(&expected), "missing tool {expected:?}");
        }

        let result = client
            .call_tool(rmcp::model::CallToolRequestParams {
                meta: None,
                name: "list_ssh_profiles".into(),
                arguments: None,
                task: None,
            })
            .await?;
        assert_ne!(result.is_error, Some(true));

        client.cancel().await?;
        let _ = server_handle.await;
        Ok(())
    }
}
