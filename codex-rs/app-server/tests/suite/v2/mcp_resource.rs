use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::to_response;
use app_test_support::write_mock_responses_config_toml;
use axum::Router;
use codex_app_server::in_process;
use codex_app_server::in_process::InProcessStartArgs;
use codex_app_server_protocol::ClientInfo;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::InitializeParams;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::McpResourceContent;
use codex_app_server_protocol::McpResourceReadParams;
use codex_app_server_protocol::McpResourceReadResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_arg0::Arg0DispatchPaths;
use codex_config::CloudConfigBundleLoader;
use codex_config::LoaderOverrides;
use codex_core::config::ConfigBuilder;
use codex_exec_server::EnvironmentManager;
use codex_feedback::CodexFeedback;
use codex_protocol::protocol::SessionSource;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use rmcp::handler::server::ServerHandler;
use rmcp::model::ProtocolVersion;
use rmcp::model::ReadResourceRequestParams;
use rmcp::model::ReadResourceResult;
use rmcp::model::ResourceContents;
use rmcp::model::ServerCapabilities;
use rmcp::model::ServerInfo;
use rmcp::service::RequestContext;
use rmcp::service::RoleServer;
use rmcp::transport::StreamableHttpServerConfig;
use rmcp::transport::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use std::collections::BTreeMap;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::timeout;

const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(10);
const TEST_RESOURCE_URI: &str = "test://codex/resource";
const TEST_BLOB_RESOURCE_URI: &str = "test://codex/resource.bin";
const TEST_RESOURCE_BLOB: &str = "YmluYXJ5LXJlc291cmNl";
const TEST_RESOURCE_TEXT: &str = "Resource body from the MCP server.";
const TEST_SERVER_NAME: &str = "resource_server";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_resource_read_returns_resource_contents() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let (mcp_server_url, mcp_server_handle) = start_resource_mcp_server().await?;

    let codex_home = TempDir::new()?;
    write_mock_responses_config_toml(
        codex_home.path(),
        &responses_server.uri(),
        &BTreeMap::new(),
        /*auto_compact_limit*/ 1024,
        /*requires_astral_auth*/ None,
        "mock_provider",
        "compact",
    )?;
    append_mcp_server_config(codex_home.path(), &mcp_server_url)?;

    let mut mcp = TestAppServer::new(codex_home.path()).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let thread_start_id = mcp
        .send_thread_start_request(ThreadStartParams {
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;
    let thread_start_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(thread_start_id)),
    )
    .await??;
    let ThreadStartResponse { thread, .. } = to_response(thread_start_resp)?;

    let read_request_id = mcp
        .send_mcp_resource_read_request(McpResourceReadParams {
            thread_id: Some(thread.id),
            server: TEST_SERVER_NAME.to_string(),
            uri: TEST_RESOURCE_URI.to_string(),
        })
        .await?;
    let read_response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(read_request_id)),
    )
    .await??;

    assert_eq!(
        to_response::<McpResourceReadResponse>(read_response)?,
        expected_resource_read_response()
    );

    mcp_server_handle.abort();
    let _ = mcp_server_handle.await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_resource_read_returns_resource_contents_without_thread() -> Result<()> {
    let (mcp_server_url, mcp_server_handle) = start_resource_mcp_server().await?;

    let codex_home = TempDir::new()?;
    std::fs::write(
        codex_home.path().join("config.toml"),
        format!(
            r#"
[mcp_servers.{TEST_SERVER_NAME}]
url = "{mcp_server_url}/mcp"
"#
        ),
    )?;

    let mut mcp = TestAppServer::new(codex_home.path()).await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let read_request_id = mcp
        .send_mcp_resource_read_request(McpResourceReadParams {
            thread_id: None,
            server: TEST_SERVER_NAME.to_string(),
            uri: TEST_RESOURCE_URI.to_string(),
        })
        .await?;
    let read_response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(read_request_id)),
    )
    .await??;

    assert_eq!(
        to_response::<McpResourceReadResponse>(read_response)?,
        expected_resource_read_response()
    );

    mcp_server_handle.abort();
    let _ = mcp_server_handle.await;
    Ok(())
}

#[tokio::test]
async fn mcp_resource_read_returns_error_for_unknown_thread() -> Result<()> {
    let codex_home = TempDir::new()?;
    let loader_overrides = LoaderOverrides::without_managed_config_for_tests();
    let config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .fallback_cwd(Some(codex_home.path().to_path_buf()))
        .loader_overrides(loader_overrides.clone())
        .build()
        .await?;
    // This negative-path test does not need the stdio subprocess; keeping it
    // in-process avoids child-process teardown timing in nextest leak detection.
    let client = in_process::start(InProcessStartArgs {
        arg0_paths: Arg0DispatchPaths::default(),
        config: Arc::new(config),
        cli_overrides: Vec::new(),
        loader_overrides,
        strict_config: false,
        cloud_config_bundle: CloudConfigBundleLoader::default(),
        thread_config_loader: Arc::new(codex_config::NoopThreadConfigLoader),
        feedback: CodexFeedback::new(),
        log_db: None,
        state_db: None,
        environment_manager: Arc::new(EnvironmentManager::default_for_tests()),
        config_warnings: Vec::new(),
        session_source: SessionSource::Cli,
        enable_astral_api_key_env: false,
        initialize: InitializeParams {
            client_info: ClientInfo {
                name: "codex-app-server-tests".to_string(),
                title: None,
                version: "0.1.0".to_string(),
            },
            capabilities: None,
        },
        channel_capacity: in_process::DEFAULT_IN_PROCESS_CHANNEL_CAPACITY,
    })
    .await?;

    let response = client
        .request(ClientRequest::McpResourceRead {
            request_id: RequestId::Integer(1),
            params: McpResourceReadParams {
                thread_id: Some("00000000-0000-4000-8000-000000000000".to_string()),
                server: TEST_SERVER_NAME.to_string(),
                uri: TEST_RESOURCE_URI.to_string(),
            },
        })
        .await;
    client.shutdown().await?;

    let error = match response? {
        Ok(result) => anyhow::bail!("expected thread-not-found error, got response: {result:?}"),
        Err(error) => error,
    };
    assert!(
        error.message.contains("thread not found"),
        "expected thread-not-found error, got: {error:?}"
    );

    Ok(())
}

async fn start_resource_mcp_server() -> Result<(String, JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let mcp_server_url = format!("http://{addr}");

    let mcp_service = StreamableHttpService::new(
        move || Ok(ResourceAppsMcpServer),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let router = Router::new().nest_service("/mcp", mcp_service);
    let mcp_server_handle = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    Ok((mcp_server_url, mcp_server_handle))
}

fn append_mcp_server_config(codex_home: &std::path::Path, mcp_server_url: &str) -> Result<()> {
    let config_path = codex_home.join("config.toml");
    let mut config_toml = std::fs::read_to_string(&config_path)?;
    config_toml.push_str(&format!(
        r#"
[mcp_servers.{TEST_SERVER_NAME}]
url = "{mcp_server_url}/mcp"
"#
    ));
    std::fs::write(config_path, config_toml)?;
    Ok(())
}

fn expected_resource_read_response() -> McpResourceReadResponse {
    McpResourceReadResponse {
        contents: vec![
            McpResourceContent::Text {
                uri: TEST_RESOURCE_URI.to_string(),
                mime_type: Some("text/markdown".to_string()),
                text: TEST_RESOURCE_TEXT.to_string(),
                meta: None,
            },
            McpResourceContent::Blob {
                uri: TEST_BLOB_RESOURCE_URI.to_string(),
                mime_type: Some("application/octet-stream".to_string()),
                blob: TEST_RESOURCE_BLOB.to_string(),
                meta: None,
            },
        ],
    }
}

#[derive(Clone, Default)]
struct ResourceAppsMcpServer;

impl ServerHandler for ResourceAppsMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_resources().build())
            .with_protocol_version(ProtocolVersion::V_2025_06_18)
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, rmcp::ErrorData> {
        let uri = request.uri;
        if uri != TEST_RESOURCE_URI {
            return Err(rmcp::ErrorData::resource_not_found(
                format!("resource not found: {uri}"),
                None,
            ));
        }

        Ok(ReadResourceResult::new(vec![
            ResourceContents::TextResourceContents {
                uri: TEST_RESOURCE_URI.to_string(),
                mime_type: Some("text/markdown".to_string()),
                text: TEST_RESOURCE_TEXT.to_string(),
                meta: None,
            },
            ResourceContents::BlobResourceContents {
                uri: TEST_BLOB_RESOURCE_URI.to_string(),
                mime_type: Some("application/octet-stream".to_string()),
                blob: TEST_RESOURCE_BLOB.to_string(),
                meta: None,
            },
        ]))
    }
}
