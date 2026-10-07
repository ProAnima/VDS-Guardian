use crate::{GuardianMcpServer, ServerConfig};
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
        "preview_capture_selection",
        "execute_capture_selection",
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
