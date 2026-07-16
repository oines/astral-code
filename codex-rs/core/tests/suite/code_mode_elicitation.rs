#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use codex_config::types::McpServerConfig;
use codex_config::types::McpServerTransportConfig;
use codex_core::config::Config;
use codex_features::Feature;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::ElicitationAction;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ReviewDecision;
use codex_protocol::request_permissions::PermissionGrantScope;
use codex_protocol::request_permissions::RequestPermissionsResponse;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::responses::ResponseMock;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_custom_tool_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::sse;
use core_test_support::skip_if_no_network;
use core_test_support::stdio_server_bin;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use core_test_support::test_codex::turn_permission_fields;
use core_test_support::wait_for_event;
use core_test_support::wait_for_event_match;
use core_test_support::wait_for_event_with_timeout;
use core_test_support::wait_for_mcp_server;
use serde_json::json;
use wiremock::MockServer;

const YIELD_TIME_MS: u64 = 1_000;
const TURN_COMPLETE_TIMEOUT: Duration = Duration::from_secs(30);

struct CodeModeElicitationHarness {
    _server: MockServer,
    test: TestCodex,
    follow_up: ResponseMock,
    turn_id: String,
}

impl CodeModeElicitationHarness {
    async fn start(
        code: &str,
        permission_profile: PermissionProfile,
        configure: impl FnOnce(&mut Config) + Send + 'static,
    ) -> Result<Self> {
        let server = responses::start_mock_server().await;
        let mut builder =
            test_codex()
                .with_model("test-gpt-5.1-codex")
                .with_config(move |config| {
                    let _ = config.features.enable(Feature::CodeMode);
                    configure(config);
                });
        let test = builder.build_with_remote_env(&server).await?;
        let follow_up = mount_code_mode_responses(&server, code).await;
        let turn_id = submit_turn(&test, permission_profile).await?;
        Ok(Self {
            _server: server,
            test,
            follow_up,
            turn_id,
        })
    }

    async fn assert_result_held(&self) {
        tokio::time::sleep(Duration::from_millis(YIELD_TIME_MS + 250)).await;
        assert!(
            self.follow_up.requests().is_empty(),
            "captured exec result should not return during a user elicitation"
        );
    }

    async fn finish(self) {
        wait_for_event_with_timeout(
            &self.test.codex,
            |event| match event {
                EventMsg::TurnComplete(event) => event.turn_id == self.turn_id,
                _ => false,
            },
            TURN_COMPLETE_TIMEOUT,
        )
        .await;
        self.follow_up.single_request();
    }
}

async fn mount_code_mode_responses(server: &MockServer, code: &str) -> ResponseMock {
    responses::mount_sse_once(
        server,
        sse(vec![
            ev_response_created("resp-1"),
            ev_custom_tool_call("call-1", "exec", code),
            ev_completed("resp-1"),
        ]),
    )
    .await;
    responses::mount_sse_once(
        server,
        sse(vec![
            ev_assistant_message("msg-1", "done"),
            ev_completed("resp-2"),
        ]),
    )
    .await
}

async fn submit_turn(test: &TestCodex, permission_profile: PermissionProfile) -> Result<String> {
    let (sandbox_policy, permission_profile) =
        turn_permission_fields(permission_profile, test.config.cwd.as_path());
    test.codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "run a code-mode tool that needs user input".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: codex_protocol::protocol::ThreadSettingsOverrides {
                approval_policy: Some(AskForApproval::OnRequest),
                sandbox_policy: Some(sandbox_policy),
                permission_profile,
                collaboration_mode: Some(codex_protocol::config_types::CollaborationMode {
                    mode: codex_protocol::config_types::ModeKind::Default,
                    settings: codex_protocol::config_types::Settings {
                        model: test.session_configured.model.clone(),
                        reasoning_effort: None,
                        developer_instructions: None,
                    },
                }),
                ..Default::default()
            },
        })
        .await?;

    Ok(wait_for_event_match(&test.codex, |event| match event {
        EventMsg::TurnStarted(event) => Some(event.turn_id.clone()),
        _ => None,
    })
    .await)
}

#[cfg_attr(windows, ignore = "no exec_command on Windows")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn code_mode_holds_yielded_result_during_command_approval() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let harness = CodeModeElicitationHarness::start(
        r#"// @exec: {"yield_time_ms": 1000}
await tools.exec_command({
  cmd: "printf code_mode_approval_marker",
  sandbox_permissions: "require_escalated",
  justification: "test command approval",
});"#,
        PermissionProfile::read_only(),
        |_| {},
    )
    .await?;
    let approval = wait_for_event_match(&harness.test.codex, |event| match event {
        EventMsg::ExecApprovalRequest(approval) => Some(approval.clone()),
        _ => None,
    })
    .await;

    harness.assert_result_held().await;
    harness
        .test
        .codex
        .submit(Op::ExecApproval {
            id: approval.effective_approval_id(),
            turn_id: Some(harness.turn_id.clone()),
            decision: ReviewDecision::Approved,
        })
        .await?;
    harness.finish().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn code_mode_holds_yielded_result_during_patch_approval() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let harness = CodeModeElicitationHarness::start(
        r#"// @exec: {"yield_time_ms": 1000}
await tools.apply_patch("*** Begin Patch\n*** Add File: code_mode_patch_approval.txt\n+held\n*** End Patch\n");"#,
        PermissionProfile::read_only(),
        |_| {},
    )
    .await?;
    let event = wait_for_event(&harness.test.codex, |event| {
        matches!(
            event,
            EventMsg::ApplyPatchApprovalRequest(_) | EventMsg::TurnComplete(_) | EventMsg::Error(_)
        )
    })
    .await;
    let EventMsg::ApplyPatchApprovalRequest(approval) = event else {
        panic!("expected apply_patch approval before turn completion, got {event:?}");
    };

    harness.assert_result_held().await;
    harness
        .test
        .codex
        .submit(Op::PatchApproval {
            id: approval.call_id,
            decision: ReviewDecision::Approved,
        })
        .await?;
    harness.finish().await;
    Ok(())
}

#[cfg_attr(
    target_os = "linux",
    ignore = "request_permissions tool integration is not supported on Linux"
)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn code_mode_holds_yielded_result_during_permission_request() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let harness = CodeModeElicitationHarness::start(
        r#"// @exec: {"yield_time_ms": 1000}
await tools.request_permissions({
  reason: "test permission request",
  permissions: { network: { enabled: true } },
});"#,
        PermissionProfile::read_only(),
        |config| {
            let _ = config.features.enable(Feature::RequestPermissionsTool);
        },
    )
    .await?;
    let request = wait_for_event(&harness.test.codex, |event| {
        matches!(
            event,
            EventMsg::RequestPermissions(_) | EventMsg::TurnComplete(_) | EventMsg::Error(_)
        )
    })
    .await;
    let EventMsg::RequestPermissions(request) = request else {
        panic!("expected request_permissions before turn completion, got {request:?}");
    };

    harness.assert_result_held().await;
    harness
        .test
        .codex
        .submit(Op::RequestPermissionsResponse {
            id: request.call_id,
            response: RequestPermissionsResponse {
                permissions: Default::default(),
                scope: PermissionGrantScope::Turn,
                strict_auto_review: false,
            },
        })
        .await?;
    harness.finish().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn code_mode_holds_nested_mcp_result_during_server_elicitation() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    let rmcp_test_server_bin = stdio_server_bin()?;
    let mut builder = test_codex()
        .with_model("test-gpt-5.1-codex")
        .with_config(move |config| {
            config
                .features
                .enable(Feature::CodeMode)
                .expect("enable code mode");
            config
                .features
                .enable(Feature::AuthElicitation)
                .expect("enable MCP elicitation capability");
            let mut servers = config.mcp_servers.get().clone();
            servers.insert(
                "rmcp".to_string(),
                McpServerConfig {
                    transport: McpServerTransportConfig::Stdio {
                        command: rmcp_test_server_bin,
                        args: Vec::new(),
                        env: None,
                        env_vars: Vec::new(),
                        cwd: None,
                    },
                    environment_id: "local".to_string(),
                    enabled: true,
                    required: false,
                    supports_parallel_tool_calls: false,
                    disabled_reason: None,
                    startup_timeout_sec: Some(Duration::from_secs(10)),
                    tool_timeout_sec: None,
                    default_tools_approval_mode: None,
                    enabled_tools: None,
                    disabled_tools: None,
                    scopes: None,
                    oauth: None,
                    oauth_resource: None,
                    tools: HashMap::new(),
                },
            );
            config
                .mcp_servers
                .set(servers)
                .expect("set MCP test server");
        });
    let test = builder.build(&server).await?;
    wait_for_mcp_server(&test.codex, "rmcp").await?;
    let response_mock = responses::mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_custom_tool_call(
                    "call-1",
                    "exec",
                    r#"// @exec: {"yield_time_ms": 1000}
const result = await tools.mcp__rmcp__elicit({});
text(JSON.stringify(result.structuredContent));"#,
                ),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                responses::ev_function_call(
                    "call-2",
                    "wait",
                    &json!({
                        "cell_id": "1",
                        "yield_time_ms": 10_000,
                    })
                    .to_string(),
                ),
                ev_completed("resp-2"),
            ]),
            sse(vec![
                ev_assistant_message("msg-1", "done"),
                ev_completed("resp-3"),
            ]),
        ],
    )
    .await;
    let turn_id = submit_turn(&test, PermissionProfile::Disabled).await?;
    let request = wait_for_event_match(&test.codex, |event| match event {
        EventMsg::ElicitationRequest(request) => Some(request.clone()),
        _ => None,
    })
    .await;

    tokio::time::sleep(Duration::from_millis(YIELD_TIME_MS + 250)).await;
    assert_eq!(
        response_mock.requests().len(),
        1,
        "nested MCP result must remain held while its elicitation is unresolved"
    );
    test.codex
        .submit(Op::ResolveElicitation {
            server_name: request.server_name,
            request_id: request.id,
            decision: ElicitationAction::Accept,
            content: Some(json!({ "answer": "accepted" })),
            meta: None,
        })
        .await?;
    wait_for_event_with_timeout(
        &test.codex,
        |event| match event {
            EventMsg::TurnComplete(event) => event.turn_id == turn_id,
            _ => false,
        },
        TURN_COMPLETE_TIMEOUT,
    )
    .await;
    let requests = response_mock.requests();
    assert_eq!(requests.len(), 3);
    let exec_output = requests[1].custom_tool_call_output("call-1");
    assert!(
        exec_output
            .to_string()
            .contains("Script running with cell ID 1")
    );
    let output = requests[2].function_call_output("call-2");
    assert!(
        output.to_string().contains("accepted"),
        "accepted MCP elicitation content should reach the nested JS promise: {output}"
    );

    Ok(())
}
