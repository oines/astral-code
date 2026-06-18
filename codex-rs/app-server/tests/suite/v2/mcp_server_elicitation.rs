use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::to_response;
use app_test_support::write_mock_responses_config_toml;
use axum::Router;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput as V2UserInput;
use core_test_support::assert_regex_match;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use rmcp::handler::server::ServerHandler;
use rmcp::model::BooleanSchema;
use rmcp::model::CallToolRequestParams;
use rmcp::model::CallToolResult;
use rmcp::model::Content;
use rmcp::model::CreateElicitationRequestParams;
use rmcp::model::ElicitationAction;
use rmcp::model::ElicitationSchema;
use rmcp::model::JsonObject;
use rmcp::model::ListToolsResult;
use rmcp::model::Meta;
use rmcp::model::PrimitiveSchema;
use rmcp::model::ServerCapabilities;
use rmcp::model::ServerInfo;
use rmcp::model::Tool;
use rmcp::model::ToolAnnotations;
use rmcp::service::RequestContext;
use rmcp::service::RoleServer;
use rmcp::transport::StreamableHttpServerConfig;
use rmcp::transport::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::timeout;

const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const CONNECTOR_ID: &str = "calendar";
const CONNECTOR_NAME: &str = "Calendar";
const TEST_SERVER_NAME: &str = "calendar_server";
const TOOL_NAMESPACE: &str = "mcp__calendar_server";
const TOOL_NAME: &str = "calendar_confirm_action";
const TOOL_CALL_ID: &str = "call-calendar-confirm";
const ELICITATION_MESSAGE: &str = "Allow this request?";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn model_initiated_mcp_elicitation_is_declined() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let tool_call_arguments = serde_json::to_string(&json!({}))?;
    let response_mock = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("resp-0"),
                responses::ev_assistant_message("msg-0", "Warmup"),
                responses::ev_completed("resp-0"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("resp-1"),
                responses::ev_function_call_with_namespace(
                    TOOL_CALL_ID,
                    TOOL_NAMESPACE,
                    TOOL_NAME,
                    &tool_call_arguments,
                ),
                responses::ev_completed("resp-1"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("resp-2"),
                responses::ev_assistant_message("msg-1", "Done"),
                responses::ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let (mcp_server_url, mcp_server_handle) = start_mcp_server().await?;

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

    let warmup_turn_start_id = mcp
        .send_turn_start_request(TurnStartParams {
            thread_id: thread.id.clone(),
            client_user_message_id: None,
            input: vec![V2UserInput::Text {
                text: "Warm up connectors.".to_string(),
                text_elements: Vec::new(),
            }],
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;
    let warmup_turn_start_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(warmup_turn_start_id)),
    )
    .await??;
    let _: TurnStartResponse = to_response(warmup_turn_start_resp)?;

    let warmup_completed = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    let warmup_completed: TurnCompletedNotification = serde_json::from_value(
        warmup_completed
            .params
            .clone()
            .expect("warmup turn/completed params"),
    )?;
    assert_eq!(warmup_completed.thread_id, thread.id);
    assert_eq!(warmup_completed.turn.status, TurnStatus::Completed);

    let turn_start_id = mcp
        .send_turn_start_request(TurnStartParams {
            thread_id: thread.id.clone(),
            client_user_message_id: None,
            input: vec![V2UserInput::Text {
                text: "Use [$calendar](app://calendar) to run the calendar tool.".to_string(),
                text_elements: Vec::new(),
            }],
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;
    let turn_start_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(turn_start_id)),
    )
    .await??;
    let TurnStartResponse { turn } = to_response(turn_start_resp)?;

    let completed = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    let completed: TurnCompletedNotification =
        serde_json::from_value(completed.params.clone().expect("turn/completed params"))?;
    assert_eq!(completed.thread_id, thread.id);
    assert_eq!(completed.turn.id, turn.id);
    assert_eq!(completed.turn.status, TurnStatus::Completed);

    let requests = response_mock.requests();
    assert_eq!(requests.len(), 3);
    let function_call_output = requests[2].function_call_output(TOOL_CALL_ID);
    assert_eq!(
        function_call_output.get("type"),
        Some(&Value::String("function_call_output".to_string()))
    );
    assert_eq!(
        function_call_output.get("call_id"),
        Some(&Value::String(TOOL_CALL_ID.to_string()))
    );
    let output = function_call_output
        .get("output")
        .and_then(Value::as_str)
        .expect("function_call_output output should be a JSON string");
    let payload = assert_regex_match(
        r#"(?s)^Wall time: [0-9]+(?:\.[0-9]+)? seconds\nOutput:\n(.*)$"#,
        output,
    )
    .get(1)
    .expect("wall-time wrapped output should include payload")
    .as_str();
    assert!(
        payload.contains("declined"),
        "unexpected tool output: {payload}"
    );

    mcp_server_handle.abort();
    let _ = mcp_server_handle.await;
    Ok(())
}

#[derive(Clone, Default)]
struct ElicitationAppsMcpServer;

impl ServerHandler for ElicitationAppsMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(rmcp::model::ProtocolVersion::V_2025_06_18)
    }

    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::ErrorData> {
        let input_schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "additionalProperties": false
        }))
        .map_err(|err| rmcp::ErrorData::internal_error(err.to_string(), None))?;

        let mut tool = Tool::new(
            Cow::Borrowed(TOOL_NAME),
            Cow::Borrowed("Confirm a calendar action."),
            Arc::new(input_schema),
        );
        tool.annotations = Some(ToolAnnotations::new().read_only(true));

        let mut meta = Meta::new();
        meta.0
            .insert("connector_id".to_string(), json!(CONNECTOR_ID));
        meta.0
            .insert("connector_name".to_string(), json!(CONNECTOR_NAME));
        tool.meta = Some(meta);

        Ok(ListToolsResult {
            tools: vec![tool],
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        _request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let requested_schema = ElicitationSchema::builder()
            .required_property("confirmed", PrimitiveSchema::Boolean(BooleanSchema::new()))
            .build()
            .map_err(|err| rmcp::ErrorData::internal_error(err.to_string(), None))?;

        let result = context
            .peer
            .create_elicitation(CreateElicitationRequestParams::FormElicitationParams {
                meta: None,
                message: ELICITATION_MESSAGE.to_string(),
                requested_schema,
            })
            .await
            .map_err(|err| rmcp::ErrorData::internal_error(err.to_string(), None))?;

        let output = match result.action {
            ElicitationAction::Accept => {
                assert_eq!(
                    result.content,
                    Some(json!({
                        "confirmed": true,
                    }))
                );
                "accepted"
            }
            ElicitationAction::Decline => "declined",
            ElicitationAction::Cancel => "cancelled",
        };

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }
}

async fn start_mcp_server() -> Result<(String, JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let mcp_service = StreamableHttpService::new(
        move || Ok(ElicitationAppsMcpServer),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );

    let router = Router::new().nest_service("/mcp", mcp_service);

    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    Ok((format!("http://{addr}"), handle))
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
