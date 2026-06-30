#![allow(clippy::expect_used)]
use codex_config::types::McpServerConfig;
use codex_config::types::McpServerTransportConfig;
use codex_core::compact::SUMMARIZATION_PROMPT;
use codex_core::compact::SUMMARY_PREFIX;
use codex_core::compact::compact_user_summary_message;
use codex_core::config::Config;
use codex_features::Feature;
use codex_login::CodexAuth;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;
use codex_models_manager::bundled_models_response;
use codex_protocol::config_types::AutoCompactTokenLimitScope;
use codex_protocol::items::TurnItem;
use codex_protocol::models::PermissionProfile;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::HookEventName;
use codex_protocol::protocol::HookRunStatus;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::ItemStartedEvent;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::RolloutLine;
use codex_protocol::protocol::WarningEvent;
use codex_protocol::user_input::UserInput;
use core_test_support::PathBufExt;
use core_test_support::context_snapshot;
use core_test_support::context_snapshot::ContextSnapshotOptions;
use core_test_support::context_snapshot::ContextSnapshotRenderMode;
use core_test_support::hooks::trust_discovered_hooks;
use core_test_support::responses::ev_reasoning_item;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::local_selections;
use core_test_support::test_codex::test_codex;
use core_test_support::test_codex::turn_permission_fields;
use core_test_support::test_path_buf;
use core_test_support::wait_for_event;
use core_test_support::wait_for_event_match;
use std::path::PathBuf;

use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_completed_with_tokens;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::mount_response_sequence;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::mount_sse_once_match;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::request_tool_names;
use core_test_support::responses::sse;
use core_test_support::responses::sse_failed;
use core_test_support::responses::sse_response;
use core_test_support::responses::start_mock_server;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;
use tokio::time::sleep;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::Respond;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

use core_test_support::stdio_server_bin;
use core_test_support::wait_for_mcp_server;
// --- Test helpers -----------------------------------------------------------

pub(super) const FIRST_REPLY: &str = "FIRST_REPLY";
pub(super) const SUMMARY_TEXT: &str = "SUMMARY_ONLY_CONTEXT";
const THIRD_USER_MSG: &str = "next turn";
const AUTO_SUMMARY_TEXT: &str = "AUTO_SUMMARY";
const FIRST_AUTO_MSG: &str = "token limit start";
const SECOND_AUTO_MSG: &str = "token limit push";
const MULTI_AUTO_MSG: &str = "multi auto";
const SECOND_LARGE_REPLY: &str = "SECOND_LARGE_REPLY";
const FIRST_AUTO_SUMMARY: &str = "FIRST_AUTO_SUMMARY";
const SECOND_AUTO_SUMMARY: &str = "SECOND_AUTO_SUMMARY";
const FINAL_REPLY: &str = "FINAL_REPLY";
const CONTEXT_LIMIT_MESSAGE: &str =
    "Your input exceeds the context window of this model. Please adjust your input and try again.";
const DUMMY_FUNCTION_NAME: &str = "test_tool";
const DUMMY_CALL_ID: &str = "call-multi-auto";
const FUNCTION_CALL_LIMIT_MSG: &str = "function call limit push";
const POST_AUTO_USER_MSG: &str = "post auto follow-up";
const PRETURN_CONTEXT_DIFF_CWD: &str = "/tmp/PRETURN_CONTEXT_DIFF_CWD";
const SESSION_MEMORY_TEST_STACK_SIZE_BYTES: usize = 16 * 1024 * 1024;

pub(super) const COMPACT_WARNING_MESSAGE: &str = "Heads up: Long threads and multiple compactions can cause the model to be less accurate. Start a new thread when possible to keep threads small and targeted.";

fn ev_shell_command_call(call_id: &str, command: &str) -> serde_json::Value {
    ev_function_call(
        call_id,
        "shell_command",
        &json!({ "command": command }).to_string(),
    )
}

fn disabled_permission_user_turn(text: impl Into<String>, cwd: PathBuf, model: String) -> Op {
    let (sandbox_policy, permission_profile) =
        turn_permission_fields(PermissionProfile::Disabled, cwd.as_path());
    Op::UserInput {
        items: vec![UserInput::Text {
            text: text.into(),
            text_elements: Vec::new(),
        }],
        final_output_json_schema: None,
        model_client_metadata: None,
        additional_context: Default::default(),
        thread_settings: codex_protocol::protocol::ThreadSettingsOverrides {
            environments: Some(local_selections(cwd.abs())),
            approval_policy: Some(AskForApproval::Never),
            sandbox_policy: Some(sandbox_policy),
            permission_profile,
            collaboration_mode: Some(codex_protocol::config_types::CollaborationMode {
                mode: codex_protocol::config_types::ModeKind::Default,
                settings: codex_protocol::config_types::Settings {
                    model,
                    reasoning_effort: None,
                    developer_instructions: None,
                },
            }),
            ..Default::default()
        },
    }
}

fn auto_summary(summary: &str) -> String {
    summary.to_string()
}

fn summary_with_prefix(summary: &str) -> String {
    format!("{SUMMARY_PREFIX}\n{summary}")
}

fn set_test_compact_prompt(config: &mut Config) {
    config.compact_prompt = Some(SUMMARIZATION_PROMPT.to_string());
}

fn enable_test_session_memory_compact(config: &mut Config) {
    config.experimental_session_memory_compact = true;
    config.session_memory_minimum_message_tokens_to_init = 10_000;
    config.session_memory_minimum_tokens_between_update = 5_000;
    config.session_memory_tool_calls_between_updates = 3;
}

fn ev_completed_with_usage(id: &str, input_tokens: i64, output_tokens: i64) -> Value {
    json!({
        "type": "response.completed",
        "response": {
            "id": id,
            "usage": {
                "input_tokens": input_tokens,
                "input_tokens_details": null,
                "output_tokens": output_tokens,
                "output_tokens_details": null,
                "total_tokens": input_tokens + output_tokens
            }
        }
    })
}

fn body_contains_text(body: &str, text: &str) -> bool {
    body.contains(&json_fragment(text))
}

async fn wait_for_request_count(
    mock: &core_test_support::responses::ResponseMock,
    expected: usize,
) -> Vec<core_test_support::responses::ResponsesRequest> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let requests = mock.requests();
        if requests.len() >= expected {
            return requests;
        }
        if Instant::now() >= deadline {
            let summaries = requests
                .iter()
                .enumerate()
                .map(|(index, request)| {
                    let body = request.body_json().to_string();
                    format!(
                        "#{index}: compact_prompt={} final_reply={}",
                        body_contains_text(&body, SUMMARIZATION_PROMPT),
                        body.contains(FINAL_REPLY)
                    )
                })
                .collect::<Vec<_>>();
            panic!(
                "expected at least {expected} requests, got {} ({})",
                requests.len(),
                summaries.join(", ")
            );
        }
        sleep(Duration::from_millis(10)).await;
    }
}

fn json_fragment(text: &str) -> String {
    serde_json::to_string(text)
        .expect("serialize text to JSON")
        .trim_matches('"')
        .to_string()
}

fn is_zstd_encoding(value: &str) -> bool {
    value
        .split(',')
        .any(|entry| entry.trim().eq_ignore_ascii_case("zstd"))
}

fn run_session_memory_test_on_large_stack<F, Fut>(name: &'static str, test: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + 'static,
{
    let handle = std::thread::Builder::new()
        .name(name.to_string())
        .stack_size(SESSION_MEMORY_TEST_STACK_SIZE_BYTES)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_stack_size(SESSION_MEMORY_TEST_STACK_SIZE_BYTES)
                .enable_all()
                .build()
                .expect("build session-memory test runtime");
            runtime.block_on(test());
        })
        .expect("spawn session-memory test thread");

    match handle.join() {
        Ok(()) => {}
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

#[test]
fn session_memory_sidechain_updates_summary_without_polluting_main_history() {
    run_session_memory_test_on_large_stack(
        "session_memory_sidechain_updates_summary_without_polluting_main_history",
        session_memory_sidechain_updates_summary_without_polluting_main_history_impl,
    );
}

async fn session_memory_sidechain_updates_summary_without_polluting_main_history_impl() {
    skip_if_no_network!();

    let server = start_mock_server().await;
    let model_provider = responses_mock_model_provider(&server);
    let rmcp_test_server_bin = stdio_server_bin().expect("resolve rmcp test server");
    let mut builder = test_codex().with_config(move |config| {
        config.model_provider = model_provider;
        enable_test_session_memory_compact(config);
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
                enabled_tools: Some(vec!["echo".to_string()]),
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
            .expect("test mcp servers should accept any configuration");
    });
    let test = builder.build(&server).await.unwrap();
    wait_for_mcp_server(&test.codex, "rmcp").await.unwrap();
    let summary_path = test
        .config
        .codex_home
        .join("session-memory")
        .join(test.session_configured.thread_id.to_string())
        .join("summary.md");
    let summary_path_str = summary_path.as_path().to_string_lossy().to_string();

    let main_turn = sse(vec![
        ev_assistant_message("m-main-1", "main turn complete"),
        ev_completed_with_tokens("r-main-1", 10_001),
    ]);
    let sidechain_edit = sse(vec![
        ev_function_call(
            "session-memory-edit",
            "Edit",
            &json!({
                "file_path": summary_path_str,
                "old_string": "_What is actively being worked on right now? Pending tasks not yet completed. Immediate next steps._\n\n# Task specification",
                "new_string": "_What is actively being worked on right now? Pending tasks not yet completed. Immediate next steps._\n- User asked the first session-memory test turn.\n\n# Task specification"
            })
            .to_string(),
        ),
        ev_completed_with_tokens("r-sidechain-edit", 10_010),
    ]);
    let next_main_turn = sse(vec![
        ev_assistant_message("m-main-2", "next main turn complete"),
        ev_completed_with_tokens("r-main-2", 10_030),
    ]);
    let mock = mount_sse_sequence(&server, vec![main_turn, sidechain_edit, next_main_turn]).await;

    test.submit_turn("first session memory turn").await.unwrap();
    let requests = wait_for_request_count(&mock, 2).await;
    assert_eq!(requests.len(), 2);
    wait_for_file_contains(
        summary_path.as_path(),
        "User asked the first session-memory test turn.",
    )
    .await;

    let main_body = requests[0].body_json();
    let sidechain_body = requests[1].body_json();
    let main_tools_schema = main_body
        .get("tools")
        .expect("main request should include tools");
    let sidechain_tools_schema = sidechain_body
        .get("tools")
        .expect("sidechain request should include tools");
    let main_tools_schema_text = main_tools_schema.to_string();
    assert!(
        main_tools_schema_text.contains("rmcp"),
        "main request should include configured MCP tools or MCP-backed discovery in its tool schema: {main_tools_schema_text}"
    );
    assert_eq!(
        sidechain_tools_schema, main_tools_schema,
        "sidechain should inherit the main request's complete model-visible tool schema"
    );
    assert_eq!(
        sidechain_body.get("parallel_tool_calls"),
        main_body.get("parallel_tool_calls"),
        "sidechain should inherit the main request's parallel tool-call setting"
    );
    let sidechain_tools = request_tool_names(&sidechain_body);
    assert!(
        sidechain_tools.contains(&"Read".to_string()),
        "Read must remain model-visible in sidechain so the tool schema stays cache-compatible"
    );
    assert!(
        sidechain_tools.contains(&"Edit".to_string()),
        "Edit must remain model-visible so the updater can modify summary.md"
    );

    let sidechain_request = sidechain_body.to_string();
    assert!(
        body_contains_text(
            &sidechain_request,
            "IMPORTANT: This message and these instructions are NOT part of the actual user conversation."
        ),
        "sidechain request should append the internal updater prompt"
    );

    let summary = fs::read_to_string(summary_path.as_path()).expect("summary file exists");
    assert!(
        summary.contains("User asked the first session-memory test turn."),
        "sidechain Edit should update summary.md"
    );

    let rollout_path = test.codex.rollout_path().expect("rollout path");
    let rollout = fs::read_to_string(rollout_path).expect("rollout exists");
    assert!(
        !rollout.contains("session notes extraction"),
        "sidechain updater prompt must not be persisted to rollout"
    );
    assert!(
        !rollout.contains("session-memory-edit"),
        "sidechain tool call must not be persisted to rollout"
    );

    test.submit_turn("second main turn").await.unwrap();
    let requests = wait_for_request_count(&mock, 3).await;
    let next_main_request = requests[2].body_json().to_string();
    assert!(
        !body_contains_text(
            &next_main_request,
            "IMPORTANT: This message and these instructions are NOT part of the actual user conversation."
        ),
        "next main request must not include sidechain updater prompt"
    );
    assert!(
        !next_main_request.contains("session-memory-edit"),
        "next main request must not include sidechain tool transcript"
    );
}

#[test]
fn session_memory_sidechain_uses_custom_template_and_prompt() {
    run_session_memory_test_on_large_stack(
        "session_memory_sidechain_uses_custom_template_and_prompt",
        session_memory_sidechain_uses_custom_template_and_prompt_impl,
    );
}

async fn session_memory_sidechain_uses_custom_template_and_prompt_impl() {
    skip_if_no_network!();

    let server = start_mock_server().await;
    let model_provider = responses_mock_model_provider(&server);
    let mut builder = test_codex().with_config(move |config| {
        config.model_provider = model_provider;
        enable_test_session_memory_compact(config);
        config.session_memory_template = Some(
            "# IM State\n_Current IM agent state_\n\n# Bridge Followups\n_Open bridge follow-ups_"
                .to_string(),
        );
        config.session_memory_update_prompt = Some(
            "CUSTOM MEMORY UPDATE\npath={{notesPath}}\nnotes={{currentNotes}}\nunknown={{missing}}"
                .to_string(),
        );
    });
    let test = builder.build(&server).await.unwrap();
    let summary_path = test
        .config
        .codex_home
        .join("session-memory")
        .join(test.session_configured.thread_id.to_string())
        .join("summary.md");
    let summary_path_str = summary_path.as_path().to_string_lossy().to_string();

    let main_turn = sse(vec![
        ev_assistant_message("m-custom-main", "main turn complete"),
        ev_completed_with_tokens("r-custom-main", 10_001),
    ]);
    let sidechain_edit = sse(vec![
        ev_function_call(
            "session-memory-custom-edit",
            "Edit",
            &json!({
                "file_path": summary_path_str,
                "old_string": "_Current IM agent state_\n\n# Bridge Followups",
                "new_string": "_Current IM agent state_\n- Bridge handoff is active.\n\n# Bridge Followups"
            })
            .to_string(),
        ),
        ev_completed_with_tokens("r-custom-sidechain-edit", 10_010),
    ]);
    let mock = mount_sse_sequence(&server, vec![main_turn, sidechain_edit]).await;

    test.submit_turn("first custom session memory turn")
        .await
        .unwrap();
    let requests = wait_for_request_count(&mock, 2).await;
    wait_for_file_contains(summary_path.as_path(), "Bridge handoff is active.").await;

    let sidechain_request = requests[1].body_json().to_string();
    assert!(
        body_contains_text(&sidechain_request, "CUSTOM MEMORY UPDATE"),
        "sidechain request should use the custom updater prompt"
    );
    assert!(
        body_contains_text(&sidechain_request, summary_path_str.as_str()),
        "custom updater prompt should receive the notes path"
    );
    assert!(
        body_contains_text(&sidechain_request, "# IM State"),
        "custom updater prompt should receive the custom template contents"
    );
}

#[test]
fn session_memory_compact_uses_summary_without_legacy_summarization_request() {
    run_session_memory_test_on_large_stack(
        "session_memory_compact_uses_summary_without_legacy_summarization_request",
        session_memory_compact_uses_summary_without_legacy_summarization_request_impl,
    );
}

async fn session_memory_compact_uses_summary_without_legacy_summarization_request_impl() {
    skip_if_no_network!();

    let server = start_mock_server().await;
    let model_provider = responses_mock_model_provider(&server);
    let mut builder = test_codex().with_config(move |config| {
        config.model_provider = model_provider;
        enable_test_session_memory_compact(config);
    });
    let test = builder.build(&server).await.unwrap();
    let summary_path = test
        .config
        .codex_home
        .join("session-memory")
        .join(test.session_configured.thread_id.to_string())
        .join("summary.md");
    let summary_path_str = summary_path.as_path().to_string_lossy().to_string();
    let state_path = summary_path.as_path().with_file_name("state.json");

    let main_turn = sse(vec![
        ev_assistant_message("m-main-before-compact", "ready to compact"),
        ev_completed_with_tokens("r-main-before-compact", 10_001),
    ]);
    let sidechain_edit = sse(vec![
        ev_function_call(
            "session-memory-compact-edit",
            "Edit",
            &json!({
                "file_path": summary_path_str,
                "old_string": "_What is actively being worked on right now? Pending tasks not yet completed. Immediate next steps._\n\n# Task specification",
                "new_string": "_What is actively being worked on right now? Pending tasks not yet completed. Immediate next steps._\n- Session memory compact has a durable summary.\n\n# Task specification"
            })
            .to_string(),
        ),
        ev_completed_with_tokens("r-sidechain-compact-edit", 10_010),
    ]);
    let post_compact_turn = sse(vec![
        ev_assistant_message("m-post-session-memory-compact", "after compact"),
        ev_completed_with_tokens("r-post-session-memory-compact", 10_030),
    ]);
    let mock =
        mount_sse_sequence(&server, vec![main_turn, sidechain_edit, post_compact_turn]).await;

    let setup_message = format!(
        "session memory compact setup\n{}\nSESSION_MEMORY_RAW_TAIL_MARKER",
        "cache-prefix-token ".repeat(3_000)
    );
    test.submit_turn(&setup_message).await.unwrap();
    wait_for_request_count(&mock, 2).await;
    wait_for_file_contains(&state_path, "\"last_summary_fingerprint\": \"").await;

    test.codex.submit(Op::Compact).await.unwrap();
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    wait_for_file_contains(&state_path, "\"last_summary_index\": null").await;

    test.submit_turn("after session memory compact")
        .await
        .unwrap();
    let requests = wait_for_request_count(&mock, 3).await;
    assert_eq!(
        requests.len(),
        3,
        "session-memory compact should not issue a legacy summarization request"
    );
    let post_compact_body = requests[2].body_json().to_string();
    assert!(
        body_contains_text(
            &post_compact_body,
            "This session is being continued from a previous conversation that ran out of context."
        ),
        "post-compact request should include session-memory summary"
    );
    assert!(
        body_contains_text(
            &post_compact_body,
            "Session memory compact has a durable summary."
        ),
        "post-compact request should include durable summary content"
    );
    assert!(
        !body_contains_text(&post_compact_body, SUMMARIZATION_PROMPT),
        "session-memory compact should not inject the legacy summarization prompt"
    );
    let context_index = post_compact_body
        .find("<environment_context>")
        .expect("post-compact request should include initial context");
    let raw_tail_index = post_compact_body
        .find("SESSION_MEMORY_RAW_TAIL_MARKER")
        .expect("post-compact request should include preserved raw tail");
    let summary_index = post_compact_body
        .find(
            "This session is being continued from a previous conversation that ran out of context.",
        )
        .expect("post-compact request should include session-memory summary");
    assert!(
        summary_index < raw_tail_index && raw_tail_index < context_index,
        "session-memory compact should keep summary before raw tail, then let the next turn reinject initial context"
    );
}

async fn wait_for_file_contains(path: &Path, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if fs::read_to_string(path).is_ok_and(|contents| contents.contains(needle)) {
            return;
        }
        if Instant::now() >= deadline {
            panic!("expected {} to contain {needle}", path.display());
        }
        sleep(Duration::from_millis(10)).await;
    }
}

fn request_body_json(request: &wiremock::Request) -> Value {
    let body = if request
        .headers
        .get("content-encoding")
        .and_then(|value| value.to_str().ok())
        .is_some_and(is_zstd_encoding)
    {
        zstd::stream::decode_all(std::io::Cursor::new(&request.body))
            .unwrap_or_else(|err| panic!("failed to decode zstd request body: {err}"))
    } else {
        request.body.clone()
    };
    serde_json::from_slice(&body).expect("request body should be JSON")
}

fn read_hook_inputs(path: &Path) -> Vec<Value> {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read hook input log {}: {err}", path.display()));
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|err| panic!("failed to parse hook input log line: {err}"))
        })
        .collect()
}

fn python_hook_command(script_path: &Path) -> String {
    format!("python3 \"{}\"", script_path.display())
}

fn write_unsupported_blocking_pre_compact_hook(home: &Path) {
    let script_path = home.join("pre_compact_block.py");
    let log_path = home.join("pre_compact_block_log.jsonl");
    let script = format!(
        r#"import json
from pathlib import Path
import sys

payload = json.load(sys.stdin)
with Path(r"{log_path}").open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(payload) + "\n")

print(json.dumps({{"decision": "block", "reason": "blocked by policy"}}))
"#,
        log_path = log_path.display(),
    );
    let hooks = json!({
        "hooks": {
            "PreCompact": [{
                "matcher": "manual",
                "hooks": [{
                    "type": "command",
                    "command": python_hook_command(&script_path),
                    "statusMessage": "checking compact policy",
                }]
            }]
        }
    });

    fs::write(&script_path, script).expect("write pre compact hook script");
    fs::write(home.join("hooks.json"), hooks.to_string()).expect("write hooks.json");
}

fn write_matching_compact_hooks(home: &Path) {
    let auto_script_path = home.join("pre_compact_auto.py");
    let auto_log_path = home.join("pre_compact_auto_log.jsonl");
    let manual_post_script_path = home.join("post_compact_manual.py");
    let manual_post_log_path = home.join("post_compact_manual_log.jsonl");
    let auto_script = format!(
        r#"import json
from pathlib import Path
import sys

payload = json.load(sys.stdin)
with Path(r"{auto_log_path}").open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(payload) + "\n")
"#,
        auto_log_path = auto_log_path.display(),
    );
    let manual_post_script = format!(
        r#"import json
from pathlib import Path
import sys

payload = json.load(sys.stdin)
with Path(r"{manual_post_log_path}").open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(payload) + "\n")
"#,
        manual_post_log_path = manual_post_log_path.display(),
    );
    let hooks = json!({
        "hooks": {
            "PreCompact": [{
                "matcher": "auto",
                "hooks": [{
                    "type": "command",
                    "command": python_hook_command(&auto_script_path),
                }]
            }],
            "PostCompact": [{
                "matcher": "manual",
                "hooks": [{
                    "type": "command",
                    "command": python_hook_command(&manual_post_script_path),
                }]
            }]
        }
    });

    fs::write(&auto_script_path, auto_script).expect("write auto pre compact hook script");
    fs::write(&manual_post_script_path, manual_post_script)
        .expect("write manual post compact hook script");
    fs::write(home.join("hooks.json"), hooks.to_string()).expect("write hooks.json");
}

fn responses_mock_model_provider(server: &MockServer) -> ModelProviderInfo {
    ModelProviderInfo {
        name: "Responses mock".into(),
        base_url: Some(format!("{}/v1", server.uri())),
        wire_api: WireApi::ChatCompletions,
        supports_websockets: false,
        ..ModelProviderInfo::default()
    }
}

fn chat_completions_mock_model_provider(server: &MockServer) -> ModelProviderInfo {
    ModelProviderInfo {
        name: "Chat completions mock".into(),
        base_url: Some(format!("{}/v1", server.uri())),
        wire_api: WireApi::ChatCompletions,
        supports_websockets: false,
        ..ModelProviderInfo::default()
    }
}

fn anthropic_messages_mock_model_provider(server: &MockServer) -> ModelProviderInfo {
    ModelProviderInfo {
        name: "Anthropic messages mock".into(),
        base_url: Some(format!("{}/v1", server.uri())),
        wire_api: WireApi::AnthropicMessages,
        supports_websockets: false,
        ..ModelProviderInfo::default()
    }
}

fn chat_completions_text_sse(id: &str, text: &str) -> String {
    [
        format!(
            "data: {}\n\n",
            json!({
                "id": id,
                "model": "astral-test-model",
                "choices": [{
                    "delta": { "role": "assistant", "content": text }
                }]
            })
        ),
        format!(
            "data: {}\n\n",
            json!({
                "choices": [{
                    "delta": {},
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 13,
                    "completion_tokens": 5,
                    "prompt_tokens_details": { "cached_tokens": 0 }
                }
            })
        ),
    ]
    .join("")
}

fn anthropic_messages_text_sse(id: &str, text: &str) -> String {
    [
        format!(
            "data: {}\n\n",
            json!({
                "type": "message_start",
                "message": { "id": id, "model": "astral-test-model" }
            })
        ),
        format!(
            "data: {}\n\n",
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": { "type": "text", "text": "" }
            })
        ),
        format!(
            "data: {}\n\n",
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": { "type": "text_delta", "text": text }
            })
        ),
        format!(
            "data: {}\n\n",
            json!({ "type": "content_block_stop", "index": 0 })
        ),
        format!(
            "data: {}\n\n",
            json!({
                "type": "message_delta",
                "delta": { "stop_reason": "end_turn" },
                "usage": { "input_tokens": 13, "output_tokens": 5 }
            })
        ),
        "data: {\"type\":\"message_stop\"}\n\n".to_string(),
    ]
    .join("")
}

async fn mount_provider_neutral_sse_sequence(
    server: &MockServer,
    endpoint_pattern: &str,
    bodies: Vec<String>,
) {
    struct SeqResponder {
        num_calls: AtomicUsize,
        responses: Vec<String>,
    }

    impl Respond for SeqResponder {
        fn respond(&self, _: &wiremock::Request) -> ResponseTemplate {
            let call_num = self.num_calls.fetch_add(1, Ordering::SeqCst);
            match self.responses.get(call_num) {
                Some(body) => ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(body.clone()),
                None => panic!("no chat completions response for {call_num}"),
            }
        }
    }

    let num_calls = bodies.len();
    let responder = SeqResponder {
        num_calls: AtomicUsize::new(0),
        responses: bodies,
    };

    Mock::given(method("POST"))
        .and(path_regex(endpoint_pattern))
        .respond_with(responder)
        .up_to_n_times(num_calls as u64)
        .expect(num_calls as u64)
        .mount(server)
        .await;
}

async fn mount_chat_completions_sse_sequence(server: &MockServer, bodies: Vec<String>) {
    mount_provider_neutral_sse_sequence(server, ".*/chat/completions$", bodies).await;
}

async fn mount_anthropic_messages_sse_sequence(server: &MockServer, bodies: Vec<String>) {
    mount_provider_neutral_sse_sequence(server, ".*/messages$", bodies).await;
}

fn model_info_with_context_window(slug: &str, context_window: i64) -> ModelInfo {
    let models_response = bundled_models_response()
        .unwrap_or_else(|err| panic!("bundled models.json should parse: {err}"));
    let mut model_info = models_response
        .models
        .into_iter()
        .find(|model| model.slug == slug)
        .unwrap_or_else(|| panic!("model `{slug}` missing from models.json"));
    model_info.context_window = Some(context_window);
    model_info
}

fn assert_pre_sampling_switch_compaction_requests(
    first: &serde_json::Value,
    compact: &serde_json::Value,
    follow_up: &serde_json::Value,
    previous_model: &str,
    next_model: &str,
) {
    assert_eq!(first["model"].as_str(), Some(previous_model));
    assert_eq!(compact["model"].as_str(), Some(previous_model));
    assert_eq!(follow_up["model"].as_str(), Some(next_model));

    let compact_body = compact.to_string();
    assert!(
        body_contains_text(&compact_body, SUMMARIZATION_PROMPT),
        "pre-sampling compact request should include summarization prompt"
    );
    assert!(
        !compact_body.contains("<model_switch>"),
        "pre-sampling compact request should strip trailing model-switch update item"
    );
    let follow_up_body = follow_up.to_string();
    assert!(
        follow_up_body.contains("<model_switch>"),
        "follow-up request after successful model-switch compaction should include model-switch update item"
    );
}

fn assert_provider_neutral_compact_request_bodies(
    label: &str,
    bodies: &[Value],
    first_user_msg: &str,
    final_user_msg: &str,
) {
    assert_eq!(bodies.len(), 3, "expected exactly three {label} requests");

    let first_body = bodies[0].to_string();
    let compact_body = bodies[1].to_string();
    let follow_up_body = bodies[2].to_string();
    let expected_summary_message = summary_with_prefix(SUMMARY_TEXT);

    assert!(
        body_contains_text(&first_body, first_user_msg),
        "first {label} request should include the original user message"
    );
    assert!(
        body_contains_text(&compact_body, SUMMARIZATION_PROMPT),
        "compact {label} request should include the summarize trigger"
    );
    assert!(
        body_contains_text(&follow_up_body, first_user_msg),
        "post-compact {label} request should retain the original user message"
    );
    assert!(
        body_contains_text(&follow_up_body, &expected_summary_message),
        "post-compact {label} request should include the compacted summary"
    );
    assert!(
        body_contains_text(&follow_up_body, final_user_msg),
        "post-compact {label} request should include the new user message"
    );
    assert!(
        !body_contains_text(&follow_up_body, FIRST_REPLY),
        "post-compact {label} request should not retain pre-compact assistant output"
    );
    assert!(
        !body_contains_text(&follow_up_body, SUMMARIZATION_PROMPT),
        "post-compact {label} request should not retain the summarize trigger"
    );
}

fn context_snapshot_options() -> ContextSnapshotOptions {
    ContextSnapshotOptions::default()
        .strip_capability_instructions()
        .render_mode(ContextSnapshotRenderMode::KindWithTextPrefix { max_chars: 64 })
}

fn format_labeled_requests_snapshot(
    scenario: &str,
    sections: &[(&str, &core_test_support::responses::ResponsesRequest)],
) -> String {
    context_snapshot::format_labeled_requests_snapshot(
        scenario,
        sections,
        &context_snapshot_options(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn summarize_context_three_requests_and_instructions() {
    skip_if_no_network!();

    // Set up a mock server that we can inspect after the run.
    let server = start_mock_server().await;

    // SSE 1: assistant replies normally so it is recorded in history.
    let sse1 = sse(vec![
        ev_assistant_message("m1", FIRST_REPLY),
        ev_completed("r1"),
    ]);

    // SSE 2: summarizer returns a summary message.
    let sse2 = sse(vec![
        ev_assistant_message("m2", SUMMARY_TEXT),
        ev_completed("r2"),
    ]);

    // SSE 3: minimal completed; we only need to capture the request body.
    let sse3 = sse(vec![ev_completed("r3")]);

    // Mount the three expected requests in sequence so the assertions below can
    // inspect them without relying on specific prompt markers.
    let request_log = mount_sse_sequence(&server, vec![sse1, sse2, sse3]).await;

    // Build config pointing to the mock server and spawn Codex.
    let model_provider = responses_mock_model_provider(&server);
    let mut builder = test_codex().with_config(move |config| {
        config.model_provider = model_provider;
        set_test_compact_prompt(config);
        config.model_auto_compact_token_limit = Some(200_000);
    });
    let test = builder.build(&server).await.unwrap();
    let codex = test.codex.clone();
    let rollout_path = test.session_configured.rollout_path.expect("rollout path");

    // 1) Normal user input – should hit server once.
    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello world".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .unwrap();
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    // 2) Summarize – second hit should include the summarization prompt.
    codex.submit(Op::Compact).await.unwrap();
    let warning_event = wait_for_event(&codex, |ev| matches!(ev, EventMsg::Warning(_))).await;
    let EventMsg::Warning(WarningEvent { message }) = warning_event else {
        panic!("expected warning event after compact");
    };
    assert_eq!(message, COMPACT_WARNING_MESSAGE);
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    // 3) Next user input – third hit; history should include only the summary.
    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: THIRD_USER_MSG.into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .unwrap();
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    // Inspect the three captured requests.
    let requests = wait_for_request_count(&request_log, 3).await;
    assert_eq!(requests.len(), 3, "expected exactly three requests");
    let body1 = requests[0].body_json();
    let body2 = requests[1].body_json();
    let body3 = requests[2].body_json();

    // Manual compact should keep the baseline developer instructions.
    let instr1 = body1.get("instructions").and_then(|v| v.as_str()).unwrap();
    let instr2 = body2.get("instructions").and_then(|v| v.as_str()).unwrap();
    assert_eq!(
        instr1, instr2,
        "manual compact should keep the standard developer instructions"
    );

    // The summarization request should include the injected user input marker.
    let body2_str = body2.to_string();
    let input2 = body2.get("input").and_then(|v| v.as_array()).unwrap();
    let has_compact_prompt = body_contains_text(&body2_str, SUMMARIZATION_PROMPT);
    assert!(
        has_compact_prompt,
        "compaction request should include the summarize trigger"
    );
    // The last item is the user message created from the injected input.
    let last2 = input2.last().unwrap();
    assert_eq!(last2.get("type").unwrap().as_str().unwrap(), "message");
    assert_eq!(last2.get("role").unwrap().as_str().unwrap(), "user");
    let text2 = last2["content"][0]["text"].as_str().unwrap();
    assert_eq!(
        text2, SUMMARIZATION_PROMPT,
        "expected summarize trigger, got `{text2}`"
    );

    // Third request must contain the refreshed instructions, compacted user history, and new user message.
    let input3 = body3.get("input").and_then(|v| v.as_array()).unwrap();

    assert!(
        input3.len() >= 3,
        "expected refreshed context and new user message in third request"
    );

    let mut messages: Vec<(String, String)> = Vec::new();
    let expected_summary_message = summary_with_prefix(SUMMARY_TEXT);

    for item in input3 {
        if let Some("message") = item.get("type").and_then(|v| v.as_str()) {
            let role = item
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let text = item
                .get("content")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|entry| entry.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            messages.push((role, text));
        }
    }

    // No previous assistant messages should remain and the new user message is present.
    let assistant_count = messages.iter().filter(|(r, _)| r == "assistant").count();
    assert_eq!(assistant_count, 0, "assistant history should be cleared");
    assert!(
        messages
            .iter()
            .any(|(r, t)| r == "user" && t == THIRD_USER_MSG),
        "third request should include the new user message"
    );
    assert!(
        messages
            .iter()
            .any(|(r, t)| r == "user" && t == "hello world"),
        "third request should include the original user message"
    );
    assert!(
        messages
            .iter()
            .any(|(r, t)| r == "user" && t == &expected_summary_message),
        "third request should include the summary message"
    );
    assert!(
        !messages
            .iter()
            .any(|(_, text)| text.contains(SUMMARIZATION_PROMPT)),
        "third request should not include the summarize trigger"
    );

    // Shut down Codex to flush rollout entries before inspecting the file.
    codex.submit(Op::Shutdown).await.unwrap();
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::ShutdownComplete)).await;

    // Verify rollout contains user-turn TurnContext entries and a Compacted entry.
    println!("rollout path: {}", rollout_path.display());
    let text = std::fs::read_to_string(&rollout_path).unwrap_or_else(|e| {
        panic!(
            "failed to read rollout file {}: {e}",
            rollout_path.display()
        )
    });
    let mut regular_turn_context_count = 0usize;
    let mut saw_compacted_summary = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(entry): Result<RolloutLine, _> = serde_json::from_str(trimmed) else {
            continue;
        };
        match entry.item {
            RolloutItem::TurnContext(_) => {
                regular_turn_context_count += 1;
            }
            RolloutItem::Compacted(ci) if ci.message == expected_summary_message => {
                saw_compacted_summary = true;
            }
            _ => {}
        }
    }

    assert_eq!(
        regular_turn_context_count, 2,
        "rollout should contain one TurnContext entry per real user turn"
    );
    assert!(
        saw_compacted_summary,
        "expected a Compacted entry containing the summarizer output"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn summarize_context_round_trips_through_chat_completions() {
    skip_if_no_network!();

    let server = start_mock_server().await;
    mount_chat_completions_sse_sequence(
        &server,
        vec![
            chat_completions_text_sse("chatcmpl-1", FIRST_REPLY),
            chat_completions_text_sse("chatcmpl-2", SUMMARY_TEXT),
            chat_completions_text_sse("chatcmpl-3", FINAL_REPLY),
        ],
    )
    .await;

    let model_provider = chat_completions_mock_model_provider(&server);
    let mut builder = test_codex().with_config(move |config| {
        config.model_provider = model_provider;
        set_test_compact_prompt(config);
        config.model_auto_compact_token_limit = Some(200_000);
    });
    let test = builder.build(&server).await.unwrap();
    let codex = test.codex.clone();

    test.submit_turn("hello world")
        .await
        .expect("submit first user turn");

    codex.submit(Op::Compact).await.unwrap();
    let warning_event = wait_for_event(&codex, |ev| matches!(ev, EventMsg::Warning(_))).await;
    let EventMsg::Warning(WarningEvent { message }) = warning_event else {
        panic!("expected warning event after compact");
    };
    assert_eq!(message, COMPACT_WARNING_MESSAGE);
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    test.submit_turn(THIRD_USER_MSG)
        .await
        .expect("submit post-compact user turn");

    let requests = server.received_requests().await.unwrap_or_default();
    let chat_bodies = requests
        .iter()
        .filter(|request| request.url.path().ends_with("/chat/completions"))
        .map(request_body_json)
        .collect::<Vec<_>>();
    assert_provider_neutral_compact_request_bodies(
        "chat completions",
        &chat_bodies,
        "hello world",
        THIRD_USER_MSG,
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn summarize_context_round_trips_through_anthropic_messages() {
    skip_if_no_network!();

    let server = start_mock_server().await;
    mount_anthropic_messages_sse_sequence(
        &server,
        vec![
            anthropic_messages_text_sse("msg_1", FIRST_REPLY),
            anthropic_messages_text_sse("msg_2", SUMMARY_TEXT),
            anthropic_messages_text_sse("msg_3", FINAL_REPLY),
        ],
    )
    .await;

    let model_provider = anthropic_messages_mock_model_provider(&server);
    let mut builder = test_codex().with_config(move |config| {
        config.model_provider = model_provider;
        set_test_compact_prompt(config);
        config.model_auto_compact_token_limit = Some(200_000);
    });
    let test = builder.build(&server).await.unwrap();
    let codex = test.codex.clone();

    test.submit_turn("hello anthropic")
        .await
        .expect("submit first user turn");

    codex.submit(Op::Compact).await.unwrap();
    let warning_event = wait_for_event(&codex, |ev| matches!(ev, EventMsg::Warning(_))).await;
    let EventMsg::Warning(WarningEvent { message }) = warning_event else {
        panic!("expected warning event after compact");
    };
    assert_eq!(message, COMPACT_WARNING_MESSAGE);
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    test.submit_turn(THIRD_USER_MSG)
        .await
        .expect("submit post-compact user turn");

    let requests = server.received_requests().await.unwrap_or_default();
    let message_bodies = requests
        .iter()
        .filter(|request| request.url.path().ends_with("/messages"))
        .map(request_body_json)
        .collect::<Vec<_>>();
    assert_provider_neutral_compact_request_bodies(
        "Anthropic messages",
        &message_bodies,
        "hello anthropic",
        THIRD_USER_MSG,
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manual_pre_compact_block_decision_does_not_block_compaction() {
    skip_if_no_network!();

    let server = start_mock_server().await;
    let first_turn = sse(vec![
        ev_assistant_message("m0", FIRST_REPLY),
        ev_completed_with_tokens("r0", /*total_tokens*/ 80),
    ]);
    let compact_turn = sse(vec![
        ev_assistant_message("m1", SUMMARY_TEXT),
        ev_completed_with_tokens("r1", /*total_tokens*/ 100),
    ]);
    let request_log = mount_sse_sequence(&server, vec![first_turn, compact_turn]).await;

    let model_provider = responses_mock_model_provider(&server);
    let mut builder = test_codex()
        .with_pre_build_hook(write_unsupported_blocking_pre_compact_hook)
        .with_config(move |config| {
            config.model_provider = model_provider;
            trust_discovered_hooks(config);
            set_test_compact_prompt(config);
        });
    let test = builder.build(&server).await.expect("create conversation");
    let codex = test.codex.clone();

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello before blocked compact".to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .expect("submit first user turn");
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex.submit(Op::Compact).await.expect("trigger compact");

    let completed = wait_for_event_match(&codex, |ev| match ev {
        EventMsg::HookCompleted(completed)
            if completed.run.event_name == HookEventName::PreCompact =>
        {
            Some(completed.clone())
        }
        _ => None,
    })
    .await;
    assert_eq!(completed.run.status, HookRunStatus::Failed);
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::Warning(_))).await;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let requests = wait_for_request_count(&request_log, 2).await;
    assert_eq!(
        requests.len(),
        2,
        "unsupported PreCompact block output should not prevent the compact request"
    );

    let hook_inputs = read_hook_inputs(&test.codex_home_path().join("pre_compact_block_log.jsonl"));
    assert_eq!(hook_inputs.len(), 1);
    let input = &hook_inputs[0];
    assert_eq!(input["hook_event_name"], "PreCompact");
    assert_eq!(input["trigger"], "manual");
    assert!(input.get("reason").is_none());
    assert!(input.get("phase").is_none());
    assert!(input.get("implementation").is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compact_hooks_respect_matchers_and_post_runs_after_compaction() {
    skip_if_no_network!();

    let server = start_mock_server().await;
    let first_turn = sse(vec![
        ev_assistant_message("m0", FIRST_REPLY),
        ev_completed_with_tokens("r0", /*total_tokens*/ 80),
    ]);
    let compact_turn = sse(vec![
        ev_assistant_message("m1", SUMMARY_TEXT),
        ev_completed_with_tokens("r1", /*total_tokens*/ 100),
    ]);
    let request_log = mount_sse_sequence(&server, vec![first_turn, compact_turn]).await;

    let model_provider = responses_mock_model_provider(&server);
    let mut builder = test_codex()
        .with_pre_build_hook(write_matching_compact_hooks)
        .with_config(move |config| {
            config.model_provider = model_provider;
            trust_discovered_hooks(config);
            set_test_compact_prompt(config);
        });
    let test = builder.build(&server).await.expect("create conversation");
    let codex = test.codex.clone();

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "hello before matched compact".to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .expect("submit first user turn");
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex.submit(Op::Compact).await.expect("trigger compact");
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::Warning(_))).await;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    assert_eq!(request_log.requests().len(), 2);
    assert!(
        !test
            .codex_home_path()
            .join("pre_compact_auto_log.jsonl")
            .exists(),
        "auto matcher should not run for manual compaction"
    );

    let hook_inputs =
        read_hook_inputs(&test.codex_home_path().join("post_compact_manual_log.jsonl"));
    assert_eq!(hook_inputs.len(), 1);
    let input = &hook_inputs[0];
    assert_eq!(input["hook_event_name"], "PostCompact");
    assert_eq!(input["trigger"], "manual");
    assert!(input.get("compact_summary").is_none());
    assert!(input.get("status").is_none());
    assert!(input.get("error").is_none());
    assert!(input.get("reason").is_none());
    assert!(input.get("phase").is_none());
    assert!(input.get("implementation").is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manual_compact_uses_custom_prompt() {
    skip_if_no_network!();

    let server = start_mock_server().await;
    let first_turn = sse(vec![
        ev_assistant_message("m0", FIRST_REPLY),
        ev_completed_with_tokens("r0", /*total_tokens*/ 80),
    ]);
    let compact_turn = sse(vec![
        ev_assistant_message("m1", SUMMARY_TEXT),
        ev_completed_with_tokens("r1", /*total_tokens*/ 100),
    ]);
    let request_log = mount_sse_sequence(&server, vec![first_turn, compact_turn]).await;

    let custom_prompt = "Use this compact prompt instead";

    let model_provider = responses_mock_model_provider(&server);
    let mut builder = test_codex().with_config(move |config| {
        config.model_provider = model_provider;
        config.compact_prompt = Some(custom_prompt.to_string());
    });
    let codex = builder
        .build(&server)
        .await
        .expect("create conversation")
        .codex;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "USER_ONE".to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .expect("submit first user turn");
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex.submit(Op::Compact).await.expect("trigger compact");
    let warning_event = wait_for_event(&codex, |ev| matches!(ev, EventMsg::Warning(_))).await;
    let EventMsg::Warning(WarningEvent { message }) = warning_event else {
        panic!("expected warning event after compact");
    };
    assert_eq!(message, COMPACT_WARNING_MESSAGE);
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let requests = request_log.requests();
    assert_eq!(
        requests.len(),
        2,
        "expected first turn and compact requests"
    );
    let body = requests[1].body_json();

    let input = body
        .get("input")
        .and_then(|v| v.as_array())
        .expect("input array");
    let mut found_custom_prompt = false;
    let mut found_default_prompt = false;

    for item in input {
        if item["type"].as_str() != Some("message") {
            continue;
        }
        let text = item["content"][0]["text"].as_str().unwrap_or_default();
        if text == custom_prompt {
            found_custom_prompt = true;
        }
        if text == SUMMARIZATION_PROMPT {
            found_default_prompt = true;
        }
    }

    let used_prompt = found_custom_prompt || found_default_prompt;
    if used_prompt {
        assert!(found_custom_prompt, "custom prompt should be injected");
        assert!(
            !found_default_prompt,
            "default prompt should be replaced when a compact prompt is used"
        );
    } else {
        assert!(
            !found_default_prompt,
            "summarization prompt should not appear if compaction omits a prompt"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manual_compact_emits_api_and_local_token_usage_events() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    // Compact run where the API reports zero tokens in usage. Our local
    // estimator should still compute a non-zero context size for the compacted
    // history.
    let sse_compact = sse(vec![
        ev_assistant_message("m1", SUMMARY_TEXT),
        ev_completed_with_tokens("r1", /*total_tokens*/ 0),
    ]);
    mount_sse_once(&server, sse_compact).await;

    let model_provider = responses_mock_model_provider(&server);
    let mut builder = test_codex().with_config(move |config| {
        config.model_provider = model_provider;
        set_test_compact_prompt(config);
    });
    let codex = builder.build(&server).await.unwrap().codex;

    // Trigger manual compact and collect TokenCount events for the compact turn.
    codex.submit(Op::Compact).await.unwrap();

    // First TokenCount: from the compact API call (usage.total_tokens = 0).
    let first = wait_for_event_match(&codex, |ev| match ev {
        EventMsg::TokenCount(tc) => tc
            .info
            .as_ref()
            .map(|info| info.last_token_usage.total_tokens),
        _ => None,
    })
    .await;

    // Second TokenCount: from the local post-compaction estimate.
    let last = wait_for_event_match(&codex, |ev| match ev {
        EventMsg::TokenCount(tc) => tc
            .info
            .as_ref()
            .map(|info| info.last_token_usage.total_tokens),
        _ => None,
    })
    .await;

    // Ensure the compact task itself completes.
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    assert_eq!(
        first, 0,
        "expected first TokenCount from compact API usage to be zero"
    );
    assert!(
        last > 0,
        "second TokenCount should reflect a non-zero estimated context size after compaction"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manual_compact_emits_context_compaction_items() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    let sse1 = sse(vec![
        ev_assistant_message("m1", FIRST_REPLY),
        ev_completed("r1"),
    ]);
    let sse2 = sse(vec![
        ev_assistant_message("m2", SUMMARY_TEXT),
        ev_completed("r2"),
    ]);
    mount_sse_sequence(&server, vec![sse1, sse2]).await;

    let model_provider = responses_mock_model_provider(&server);
    let mut builder = test_codex().with_config(move |config| {
        config.model_provider = model_provider;
        set_test_compact_prompt(config);
    });
    let codex = builder.build(&server).await.unwrap().codex;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "manual compact".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .unwrap();
    wait_for_event(&codex, |event| matches!(event, EventMsg::TurnComplete(_))).await;

    codex.submit(Op::Compact).await.unwrap();

    let mut started_item = None;
    let mut completed_item = None;
    let mut legacy_event = false;
    let mut saw_turn_complete = false;

    while !saw_turn_complete || started_item.is_none() || completed_item.is_none() || !legacy_event
    {
        let event = codex.next_event().await.unwrap();
        match event.msg {
            EventMsg::ItemStarted(ItemStartedEvent {
                item: TurnItem::ContextCompaction(item),
                ..
            }) => {
                started_item = Some(item);
            }
            EventMsg::ItemCompleted(ItemCompletedEvent {
                item: TurnItem::ContextCompaction(item),
                ..
            }) => {
                completed_item = Some(item);
            }
            EventMsg::ContextCompacted(_) => {
                legacy_event = true;
            }
            EventMsg::TurnComplete(_) => {
                saw_turn_complete = true;
            }
            _ => {}
        }
    }

    let started_item = started_item.expect("context compaction item started");
    let completed_item = completed_item.expect("context compaction item completed");
    assert_eq!(started_item.id, completed_item.id);
    assert!(legacy_event);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multiple_auto_compact_per_task_runs_after_token_limit_hit() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    let responses_mock_provider_name = responses_mock_model_provider(&server).name;
    let codex = test_codex()
        .with_config(move |config| {
            config.model_provider.name = responses_mock_provider_name;
        })
        .build(&server)
        .await
        .expect("build codex")
        .codex;

    // user message
    let user_message = "create an app";

    // Prepare the mock responses from the model

    // summary texts from model
    let first_summary_text = "The task is to create an app. I started to create a react app.";
    let second_summary_text = "The task is to create an app. I started to create a react app. then I realized that I need to create a node app.";
    let third_summary_text = "The task is to create an app. I started to create a react app. then I realized that I need to create a node app. then I realized that I need to create a python app.";
    // summary texts with prefix
    let prefixed_first_auto_summary = compact_user_summary_message(
        first_summary_text,
        /*suppress_follow_up_questions*/ true,
    );
    let prefixed_second_auto_summary = compact_user_summary_message(
        second_summary_text,
        /*suppress_follow_up_questions*/ true,
    );
    let prefixed_third_auto_summary = compact_user_summary_message(
        third_summary_text,
        /*suppress_follow_up_questions*/ true,
    );
    // token used count after long work
    let token_count_used = 270_000;
    // token used count after compaction
    let token_count_used_after_compaction = 80000;

    // mock responses from the model

    let reasoning_response_1 = ev_reasoning_item("m1", &["I will create a react app"], &[]);
    let encrypted_content_1 = reasoning_response_1["item"]["encrypted_content"]
        .as_str()
        .unwrap();

    // first chunk of work
    let model_reasoning_response_1_sse = sse(vec![
        reasoning_response_1.clone(),
        ev_shell_command_call("r1-shell", "echo make-react"),
        ev_completed_with_tokens("r1", token_count_used),
    ]);

    // first compaction response
    let model_compact_response_1_sse = sse(vec![
        ev_assistant_message("m2", first_summary_text),
        ev_completed_with_tokens("r2", token_count_used_after_compaction),
    ]);

    let reasoning_response_2 = ev_reasoning_item("m3", &["I will create a node app"], &[]);
    let encrypted_content_2 = reasoning_response_2["item"]["encrypted_content"]
        .as_str()
        .unwrap();

    // second chunk of work
    let model_reasoning_response_2_sse = sse(vec![
        reasoning_response_2.clone(),
        ev_shell_command_call("r3-shell", "echo make-node"),
        ev_completed_with_tokens("r3", token_count_used),
    ]);

    // second compaction response
    let model_compact_response_2_sse = sse(vec![
        ev_assistant_message("m4", second_summary_text),
        ev_completed_with_tokens("r4", token_count_used_after_compaction),
    ]);

    let reasoning_response_3 = ev_reasoning_item("m6", &["I will create a python app"], &[]);
    let encrypted_content_3 = reasoning_response_3["item"]["encrypted_content"]
        .as_str()
        .unwrap();

    // third chunk of work
    let model_reasoning_response_3_sse = sse(vec![
        ev_reasoning_item("m6", &["I will create a python app"], &[]),
        ev_shell_command_call("r6-shell", "echo make-python"),
        ev_completed_with_tokens("r6", token_count_used),
    ]);

    // third compaction response
    let model_compact_response_3_sse = sse(vec![
        ev_assistant_message("m7", third_summary_text),
        ev_completed_with_tokens("r7", token_count_used_after_compaction),
    ]);

    // final response
    let model_final_response_sse = sse(vec![
        ev_assistant_message(
            "m8",
            "The task is to create an app. I started to create a react app. then I realized that I need to create a node app. then I realized that I need to create a python app.",
        ),
        ev_completed_with_tokens("r8", token_count_used_after_compaction + 1000),
    ]);

    // mount the mock responses from the model
    let bodies = vec![
        model_reasoning_response_1_sse,
        model_compact_response_1_sse,
        model_reasoning_response_2_sse,
        model_compact_response_2_sse,
        model_reasoning_response_3_sse,
        model_compact_response_3_sse,
        model_final_response_sse,
    ];
    let request_log = mount_sse_sequence(&server, bodies).await;

    // Start the conversation with the user message
    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: user_message.into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .expect("submit user input");
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    // collect the requests payloads from the model
    let requests_payloads = request_log.requests();
    let body = requests_payloads[0].body_json();
    let input = body.get("input").and_then(|v| v.as_array()).unwrap();

    fn strip_agents_parts_from_user_message(
        value: &serde_json::Value,
    ) -> Option<serde_json::Value> {
        let content = value
            .get("content")
            .and_then(|content| content.as_array())?;
        let filtered_content = content
            .iter()
            .filter(|item| {
                !item
                    .get("text")
                    .and_then(|text| text.as_str())
                    .is_some_and(|text| text.starts_with("# AGENTS.md instructions for "))
            })
            .cloned()
            .collect::<Vec<_>>();
        if filtered_content.is_empty() {
            return None;
        }
        let mut normalized = value.clone();
        normalized["content"] = serde_json::Value::Array(filtered_content);
        Some(normalized)
    }

    fn normalize_inputs(values: &[serde_json::Value]) -> Vec<serde_json::Value> {
        values
            .iter()
            .filter_map(|value| {
                let value_type = value.get("type").and_then(|ty| ty.as_str());
                if value_type.is_some_and(|ty| ty == "function_call_output" || ty == "reasoning") {
                    return None;
                }

                // Ignore cached prefix messages (project docs + permissions) since they are not
                // relevant to compaction behavior and can change as bundled prompts evolve.
                let role = value.get("role").and_then(|role| role.as_str());
                if role == Some("developer") {
                    return None;
                }
                if role == Some("user") {
                    return strip_agents_parts_from_user_message(value);
                }
                Some(value.clone())
            })
            .collect()
    }

    let initial_input = normalize_inputs(input);
    let environment_message = initial_input[0]["content"][0]["text"].as_str().unwrap();

    // test 1: after compaction, we should have one environment message, one user message, and one user message with summary prefix
    let compaction_indices = [2, 4, 6];
    let expected_summaries = [
        prefixed_first_auto_summary.as_str(),
        prefixed_second_auto_summary.as_str(),
        prefixed_third_auto_summary.as_str(),
    ];
    for (i, expected_summary) in compaction_indices.into_iter().zip(expected_summaries) {
        let body = requests_payloads.clone()[i].body_json();
        let input = body.get("input").and_then(|v| v.as_array()).unwrap();
        let input = normalize_inputs(input);
        assert_eq!(input.len(), 3);
        let environment_message = input[0]["content"][0]["text"].as_str().unwrap();
        let user_message_received = input[1]["content"][0]["text"].as_str().unwrap();
        let summary_message = input[2]["content"][0]["text"].as_str().unwrap();
        assert_eq!(environment_message, environment_message);
        assert_eq!(user_message_received, user_message);
        assert_eq!(
            summary_message, expected_summary,
            "compaction request at index {i} should include the prefixed summary"
        );
    }

    // test 2: the expected requests inputs should be as follows:
    let expected_requests_inputs = json!([
    [
        // 0: first request of the user message.
      {
        "content": [
          {
            "text": environment_message,
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      },
      {
        "content": [
          {
            "text": "create an app",
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      }
    ]
    ,
    [
        // 1: first automatic compaction request.
      {
        "content": [
          {
            "text": environment_message,
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      },
      {
        "content": [
          {
            "text": "create an app",
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      },
      {
        "content": null,
        "encrypted_content": encrypted_content_1,
        "summary": [
          {
            "text": "I will create a react app",
            "type": "summary_text"
          }
        ],
        "type": "reasoning"
      },
      {
        "arguments": "{\"command\":\"echo make-react\"}",
        "call_id": "r1-shell",
        "name": "shell_command",
        "type": "function_call"
      },
      {
        "call_id": "r1-shell",
        "output": "execution error: Io(Os { code: 2, kind: NotFound, message: \"No such file or directory\" })",
        "type": "function_call_output"
      },
      {
        "content": [
          {
            "text": SUMMARIZATION_PROMPT,
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      }
    ]
    ,
    [
      // 2: request after first automatic compaction.
      {
        "content": [
          {
            "text": environment_message,
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      },
      {
        "content": [
          {
            "text": "create an app",
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      },
      {
        "content": [
          {
            "text": prefixed_first_auto_summary.clone(),
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      }
    ]
    ,
    [
        // 3: request for second automatic compaction.
      {
        "content": [
          {
            "text": environment_message,
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      },
      {
        "content": [
          {
            "text": "create an app",
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      },
      {
        "content": [
          {
            "text": prefixed_first_auto_summary.clone(),
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      },
      {
        "content": null,
        "encrypted_content": encrypted_content_2,
        "summary": [
          {
            "text": "I will create a node app",
            "type": "summary_text"
          }
        ],
        "type": "reasoning"
      },
      {
        "arguments": "{\"command\":\"echo make-node\"}",
        "call_id": "r3-shell",
        "name": "shell_command",
        "type": "function_call"
      },
      {
        "call_id": "r3-shell",
        "output": "execution error: Io(Os { code: 2, kind: NotFound, message: \"No such file or directory\" })",
        "type": "function_call_output"
      },
      {
        "content": [
          {
            "text": SUMMARIZATION_PROMPT,
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      }
    ]
    ,
    // 4: request after second automatic compaction.
    [
      {
        "content": [
          {
            "text": environment_message,
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      },
      {
        "content": [
          {
            "text": "create an app",
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      },
      {
        "content": [
          {
            "text": prefixed_second_auto_summary.clone(),
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      }
    ]
    ,
    [
      // 5: request for third automatic compaction.
      {
        "content": [
          {
            "text": environment_message,
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      },
      {
        "content": [
          {
            "text": "create an app",
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      },
      {
        "content": [
          {
            "text": prefixed_second_auto_summary.clone(),
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      },
      {
        "content": null,
        "encrypted_content": encrypted_content_3,
        "summary": [
          {
            "text": "I will create a python app",
            "type": "summary_text"
          }
        ],
        "type": "reasoning"
      },
      {
        "arguments": "{\"command\":\"echo make-python\"}",
        "call_id": "r6-shell",
        "name": "shell_command",
        "type": "function_call"
      },
      {
        "call_id": "r6-shell",
        "output": "execution error: Io(Os { code: 2, kind: NotFound, message: \"No such file or directory\" })",
        "type": "function_call_output"
      },
      {
        "content": [
          {
            "text": SUMMARIZATION_PROMPT,
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      }
    ]
    ,
    [
      {
        // 6: request after third automatic compaction.
        "content": [
          {
            "text": environment_message,
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      },
      {
        "content": [
          {
            "text": "create an app",
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      },
      {
        "content": [
          {
            "text": prefixed_third_auto_summary.clone(),
            "type": "input_text"
          }
        ],
        "role": "user",
        "type": "message"
      }
    ]
    ]);

    for (i, request) in requests_payloads.iter().enumerate() {
        let body = request.body_json();
        let input = body.get("input").and_then(|v| v.as_array()).unwrap();
        let expected_input = expected_requests_inputs[i].as_array().unwrap();
        assert_eq!(normalize_inputs(input), normalize_inputs(expected_input));
    }

    // test 3: the number of requests should be 7
    assert_eq!(requests_payloads.len(), 7);
}

// Windows CI only: bump to 4 workers to prevent SSE/event starvation and test timeouts.
#[cfg_attr(windows, tokio::test(flavor = "multi_thread", worker_threads = 4))]
#[cfg_attr(not(windows), tokio::test(flavor = "multi_thread", worker_threads = 2))]
async fn auto_compact_runs_after_token_limit_hit() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    let sse1 = sse(vec![
        ev_assistant_message("m1", FIRST_REPLY),
        ev_completed_with_tokens("r1", /*total_tokens*/ 70_000),
    ]);

    let sse2 = sse(vec![
        ev_assistant_message("m2", "SECOND_REPLY"),
        ev_completed_with_tokens("r2", /*total_tokens*/ 330_000),
    ]);

    let sse3 = sse(vec![
        ev_assistant_message("m3", AUTO_SUMMARY_TEXT),
        ev_completed_with_tokens("r3", /*total_tokens*/ 200),
    ]);
    let sse4 = sse(vec![
        ev_assistant_message("m4", FINAL_REPLY),
        ev_completed_with_tokens("r4", /*total_tokens*/ 120),
    ]);
    let prefixed_auto_summary = AUTO_SUMMARY_TEXT;

    let request_log = mount_sse_sequence(&server, vec![sse1, sse2, sse3, sse4]).await;

    let model_provider = responses_mock_model_provider(&server);

    let mut builder = test_codex().with_config(move |config| {
        config.model_provider = model_provider;
        set_test_compact_prompt(config);
        config.model_auto_compact_token_limit = Some(200_000);
    });
    let codex = builder.build(&server).await.unwrap().codex;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: FIRST_AUTO_MSG.into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .unwrap();

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: SECOND_AUTO_MSG.into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .unwrap();

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: POST_AUTO_USER_MSG.into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .unwrap();

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let requests = request_log.requests();
    let request_bodies: Vec<String> = requests
        .iter()
        .map(|request| request.body_json().to_string())
        .collect();
    assert_eq!(
        request_bodies.len(),
        4,
        "expected user turns, a compaction request, and the follow-up turn; got {}",
        request_bodies.len()
    );
    let auto_compact_count = request_bodies
        .iter()
        .filter(|body| body_contains_text(body, SUMMARIZATION_PROMPT))
        .count();
    assert_eq!(
        auto_compact_count, 1,
        "expected exactly one auto compact request"
    );
    let auto_compact_index = request_bodies
        .iter()
        .enumerate()
        .find_map(|(idx, body)| body_contains_text(body, SUMMARIZATION_PROMPT).then_some(idx))
        .expect("auto compact request missing");
    assert_eq!(
        auto_compact_index, 2,
        "auto compact should add a third request"
    );

    let follow_up_index = request_bodies
        .iter()
        .enumerate()
        .rev()
        .find_map(|(idx, body)| {
            (body.contains(POST_AUTO_USER_MSG) && !body_contains_text(body, SUMMARIZATION_PROMPT))
                .then_some(idx)
        })
        .expect("follow-up request missing");
    assert_eq!(follow_up_index, 3, "follow-up request should be last");

    let body_first = requests[0].body_json();
    let body_auto = requests[auto_compact_index].body_json();
    let body_follow_up = requests[follow_up_index].body_json();
    let instructions = body_auto
        .get("instructions")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let baseline_instructions = body_first
        .get("instructions")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    assert_eq!(
        instructions, baseline_instructions,
        "auto compact should keep the standard developer instructions",
    );

    let input_auto = body_auto.get("input").and_then(|v| v.as_array()).unwrap();
    let last_auto = input_auto
        .last()
        .expect("auto compact request should append a user message");
    assert_eq!(
        last_auto.get("type").and_then(|v| v.as_str()),
        Some("message")
    );
    assert_eq!(last_auto.get("role").and_then(|v| v.as_str()), Some("user"));
    let last_text = last_auto
        .get("content")
        .and_then(|v| v.as_array())
        .and_then(|items| items.first())
        .and_then(|item| item.get("text"))
        .and_then(|text| text.as_str())
        .unwrap_or_default();
    assert_eq!(
        last_text, SUMMARIZATION_PROMPT,
        "auto compact should send the summarization prompt as a user message",
    );

    let input_follow_up = body_follow_up
        .get("input")
        .and_then(|v| v.as_array())
        .unwrap();
    let user_texts: Vec<String> = input_follow_up
        .iter()
        .filter(|item| item.get("type").and_then(|v| v.as_str()) == Some("message"))
        .filter(|item| item.get("role").and_then(|v| v.as_str()) == Some("user"))
        .filter_map(|item| {
            item.get("content")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|entry| entry.get("text"))
                .and_then(|v| v.as_str())
                .map(std::string::ToString::to_string)
        })
        .collect();
    assert!(
        user_texts.iter().any(|text| text == FIRST_AUTO_MSG),
        "auto compact follow-up request should include the first user message"
    );
    assert!(
        user_texts.iter().any(|text| text == SECOND_AUTO_MSG),
        "auto compact follow-up request should include the second user message"
    );
    assert!(
        user_texts.iter().any(|text| text == POST_AUTO_USER_MSG),
        "auto compact follow-up request should include the new user message"
    );
    assert!(
        user_texts
            .iter()
            .any(|text| text.contains(prefixed_auto_summary)),
        "auto compact follow-up request should include the summary message"
    );
}

// Windows CI only: bump to 4 workers to prevent SSE/event starvation and test timeouts.
#[cfg_attr(windows, tokio::test(flavor = "multi_thread", worker_threads = 4))]
#[cfg_attr(not(windows), tokio::test(flavor = "multi_thread", worker_threads = 2))]
async fn auto_compact_emits_context_compaction_items() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    let sse1 = sse(vec![
        ev_assistant_message("m1", FIRST_REPLY),
        ev_completed_with_tokens("r1", /*total_tokens*/ 70_000),
    ]);
    let sse2 = sse(vec![
        ev_assistant_message("m2", "SECOND_REPLY"),
        ev_completed_with_tokens("r2", /*total_tokens*/ 330_000),
    ]);
    let sse3 = sse(vec![
        ev_assistant_message("m3", AUTO_SUMMARY_TEXT),
        ev_completed_with_tokens("r3", /*total_tokens*/ 200),
    ]);
    let sse4 = sse(vec![
        ev_assistant_message("m4", FINAL_REPLY),
        ev_completed_with_tokens("r4", /*total_tokens*/ 120),
    ]);

    mount_sse_sequence(&server, vec![sse1, sse2, sse3, sse4]).await;

    let model_provider = responses_mock_model_provider(&server);
    let mut builder = test_codex().with_config(move |config| {
        config.model_provider = model_provider;
        set_test_compact_prompt(config);
        config.model_auto_compact_token_limit = Some(200_000);
    });
    let codex = builder.build(&server).await.unwrap().codex;

    let mut started_item = None;
    let mut completed_item = None;
    let mut legacy_event = false;

    for user in [FIRST_AUTO_MSG, SECOND_AUTO_MSG, POST_AUTO_USER_MSG] {
        codex
            .submit(Op::UserInput {
                items: vec![UserInput::Text {
                    text: user.into(),
                    text_elements: Vec::new(),
                }],
                final_output_json_schema: None,
                model_client_metadata: None,
                additional_context: Default::default(),
                thread_settings: Default::default(),
            })
            .await
            .unwrap();

        loop {
            let event = codex.next_event().await.unwrap();
            match event.msg {
                EventMsg::ItemStarted(ItemStartedEvent {
                    item: TurnItem::ContextCompaction(item),
                    ..
                }) => {
                    started_item = Some(item);
                }
                EventMsg::ItemCompleted(ItemCompletedEvent {
                    item: TurnItem::ContextCompaction(item),
                    ..
                }) => {
                    completed_item = Some(item);
                }
                EventMsg::ContextCompacted(_) => {
                    legacy_event = true;
                }
                EventMsg::TurnComplete(_) if !event.id.starts_with("auto-compact-") => {
                    break;
                }
                _ => {}
            }
        }
    }

    let started_item = started_item.expect("context compaction item started");
    let completed_item = completed_item.expect("context compaction item completed");
    assert_eq!(started_item.id, completed_item.id);
    assert!(legacy_event);
}

// Windows CI only: bump to 4 workers to prevent SSE/event starvation and test timeouts.
#[cfg_attr(windows, tokio::test(flavor = "multi_thread", worker_threads = 4))]
#[cfg_attr(not(windows), tokio::test(flavor = "multi_thread", worker_threads = 2))]
async fn auto_compact_starts_after_turn_started() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    let sse1 = sse(vec![
        ev_assistant_message("m1", FIRST_REPLY),
        ev_completed_with_tokens("r1", /*total_tokens*/ 70_000),
    ]);
    let sse2 = sse(vec![
        ev_assistant_message("m2", "SECOND_REPLY"),
        ev_completed_with_tokens("r2", /*total_tokens*/ 330_000),
    ]);
    let sse3 = sse(vec![
        ev_assistant_message("m3", AUTO_SUMMARY_TEXT),
        ev_completed_with_tokens("r3", /*total_tokens*/ 200),
    ]);
    let sse4 = sse(vec![
        ev_assistant_message("m4", FINAL_REPLY),
        ev_completed_with_tokens("r4", /*total_tokens*/ 120),
    ]);

    mount_sse_sequence(&server, vec![sse1, sse2, sse3, sse4]).await;

    let model_provider = responses_mock_model_provider(&server);
    let mut builder = test_codex().with_config(move |config| {
        config.model_provider = model_provider;
        set_test_compact_prompt(config);
        config.model_auto_compact_token_limit = Some(200_000);
    });
    let codex = builder.build(&server).await.unwrap().codex;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: FIRST_AUTO_MSG.into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .unwrap();
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: SECOND_AUTO_MSG.into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .unwrap();
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: POST_AUTO_USER_MSG.into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .unwrap();

    let first = wait_for_event_match(&codex, |ev| match ev {
        EventMsg::TurnStarted(_) => Some("turn"),
        EventMsg::ItemStarted(ItemStartedEvent {
            item: TurnItem::ContextCompaction(_),
            ..
        }) => Some("compaction"),
        _ => None,
    })
    .await;
    assert_eq!(first, "turn", "compaction started before turn started");

    wait_for_event(&codex, |ev| {
        matches!(
            ev,
            EventMsg::ItemStarted(ItemStartedEvent {
                item: TurnItem::ContextCompaction(_),
                ..
            })
        )
    })
    .await;

    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_compact_runs_after_resume_when_token_usage_is_over_limit() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    let limit = 200_000;
    let over_limit_tokens = 250_000;
    let local_summary = "LOCAL_COMPACT_SUMMARY";

    let mut builder = test_codex().with_config(move |config| {
        set_test_compact_prompt(config);
        config.model_auto_compact_token_limit = Some(limit);
    });
    let initial = builder.build(&server).await.unwrap();
    let home = initial.home.clone();
    let rollout_path = initial
        .session_configured
        .rollout_path
        .clone()
        .expect("rollout path");

    // A single over-limit completion should not auto-compact until the next user message.
    mount_sse_once(
        &server,
        sse(vec![
            ev_assistant_message("m1", FIRST_REPLY),
            ev_completed_with_tokens("r1", over_limit_tokens),
        ]),
    )
    .await;
    initial.submit_turn("OVER_LIMIT_TURN").await.unwrap();

    let mut resume_builder = test_codex().with_config(move |config| {
        set_test_compact_prompt(config);
        config.model_auto_compact_token_limit = Some(limit);
    });
    let resumed = resume_builder
        .resume(&server, home, rollout_path)
        .await
        .unwrap();

    let follow_up_user = "AFTER_RESUME_USER";
    let auto_compact_turn = sse(vec![
        ev_assistant_message("compact-summary", local_summary),
        ev_completed("compact-response"),
    ]);
    let follow_up_turn = sse(vec![
        ev_assistant_message("m2", FINAL_REPLY),
        ev_completed("r2"),
    ]);
    let request_log = mount_sse_sequence(&server, vec![auto_compact_turn, follow_up_turn]).await;

    resumed
        .codex
        .submit(disabled_permission_user_turn(
            follow_up_user,
            resumed.cwd.path().to_path_buf(),
            resumed.session_configured.model.clone(),
        ))
        .await
        .unwrap();

    wait_for_event(&resumed.codex, |event| {
        matches!(event, EventMsg::ContextCompacted(_))
    })
    .await;
    wait_for_event(&resumed.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let requests = request_log.requests();
    assert_eq!(
        requests.len(),
        2,
        "resumed turn should compact once before sampling"
    );
    assert!(
        body_contains_text(&requests[0].body_json().to_string(), SUMMARIZATION_PROMPT),
        "first resumed request should be the local compact request"
    );
    let follow_up_body = requests[1].body_json().to_string();
    assert!(
        follow_up_body.contains(follow_up_user) && follow_up_body.contains(local_summary),
        "follow-up request should include the local compact summary"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_sampling_compact_runs_on_switch_to_smaller_context_model() {
    skip_if_no_network!();

    let server = MockServer::start().await;
    let previous_model = "gpt-5.3-codex";
    let next_model = "gpt-5.2";

    let model_catalog = ModelsResponse {
        models: vec![
            model_info_with_context_window(previous_model, /*context_window*/ 273_000),
            model_info_with_context_window(next_model, /*context_window*/ 125_000),
        ],
    };
    let request_log = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_assistant_message("m1", "before switch"),
                ev_completed_with_tokens("r1", /*total_tokens*/ 130_000),
            ]),
            sse(vec![
                ev_assistant_message("m2", "PRE_SAMPLING_SUMMARY"),
                ev_completed_with_tokens("r2", /*total_tokens*/ 10),
            ]),
            sse(vec![
                ev_assistant_message("m3", "after switch"),
                ev_completed_with_tokens("r3", /*total_tokens*/ 100),
            ]),
        ],
    )
    .await;

    let model_provider = responses_mock_model_provider(&server);
    let mut builder = test_codex()
        .with_auth(CodexAuth::create_dummy_api_key_auth_for_testing())
        .with_model(previous_model)
        .with_config(move |config| {
            config.model_provider = model_provider;
            config.model_catalog = Some(model_catalog);
            set_test_compact_prompt(config);
        });
    let test = builder.build(&server).await.expect("build test codex");

    test.codex
        .submit(disabled_permission_user_turn(
            "before switch",
            test.cwd.path().to_path_buf(),
            previous_model.to_string(),
        ))
        .await
        .expect("submit first user turn");
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    test.codex
        .submit(disabled_permission_user_turn(
            "after switch",
            test.cwd.path().to_path_buf(),
            next_model.to_string(),
        ))
        .await
        .expect("submit second user turn");
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let requests = wait_for_request_count(&request_log, 3).await;
    assert_eq!(
        requests.len(),
        3,
        "expected user, compact, and follow-up requests"
    );
    assert_pre_sampling_switch_compaction_requests(
        &requests[0].body_json(),
        &requests[1].body_json(),
        &requests[2].body_json(),
        previous_model,
        next_model,
    );

    insta::assert_snapshot!(
        "pre_sampling_model_switch_compaction_shapes",
        format_labeled_requests_snapshot(
            "Pre-sampling compaction on model switch to a smaller context window: current behavior compacts using prior-turn history only (incoming user message excluded), and the follow-up request carries compacted history plus the new user message.",
            &[
                ("Initial Request (Previous Model)", &requests[0]),
                ("Pre-sampling Compaction Request", &requests[1]),
                (
                    "Post-Compaction Follow-up Request (Next Model)",
                    &requests[2]
                ),
            ]
        )
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn body_after_prefix_model_switch_budget_compacts_with_next_model() {
    skip_if_no_network!();

    let server = MockServer::start().await;
    let previous_model = "gpt-5.3-codex";
    let next_model = "gpt-5.2";

    let model_catalog = ModelsResponse {
        models: vec![
            model_info_with_context_window(previous_model, /*context_window*/ 273_000),
            model_info_with_context_window(next_model, /*context_window*/ 125_000),
        ],
    };
    let request_log = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_assistant_message("m1", "before switch"),
                ev_completed_with_usage("r1", /*input_tokens*/ 100, /*output_tokens*/ 50),
            ]),
            sse(vec![
                ev_assistant_message("m2", "BODY_BUDGET_SUMMARY"),
                ev_completed_with_tokens("r2", /*total_tokens*/ 10),
            ]),
            sse(vec![
                ev_assistant_message("m3", "after switch"),
                ev_completed_with_tokens("r3", /*total_tokens*/ 100),
            ]),
        ],
    )
    .await;

    let model_provider = responses_mock_model_provider(&server);
    let mut builder = test_codex()
        .with_auth(CodexAuth::create_dummy_api_key_auth_for_testing())
        .with_model(previous_model)
        .with_config(move |config| {
            config.model_provider = model_provider;
            config.model_catalog = Some(model_catalog);
            set_test_compact_prompt(config);
            let _ = config.features.enable(Feature::RemoteModels);
            config.model_auto_compact_token_limit = Some(20);
            config.model_auto_compact_token_limit_scope =
                AutoCompactTokenLimitScope::BodyAfterPrefix;
        });
    let test = builder.build(&server).await.expect("build test codex");

    test.codex
        .submit(disabled_permission_user_turn(
            "before switch",
            test.cwd.path().to_path_buf(),
            previous_model.to_string(),
        ))
        .await
        .expect("submit first user turn");
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    test.codex
        .submit(disabled_permission_user_turn(
            "after switch",
            test.cwd.path().to_path_buf(),
            next_model.to_string(),
        ))
        .await
        .expect("submit second user turn");
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let requests = wait_for_request_count(&request_log, 3).await;
    assert_eq!(
        requests.len(),
        3,
        "expected user, compact, and follow-up requests"
    );
    assert_eq!(
        requests[0].body_json()["model"].as_str(),
        Some(previous_model)
    );
    assert_eq!(requests[1].body_json()["model"].as_str(), Some(next_model));
    assert_eq!(requests[2].body_json()["model"].as_str(), Some(next_model));
    assert!(
        body_contains_text(&requests[1].body_json().to_string(), SUMMARIZATION_PROMPT),
        "body-budget compaction request should include summarization prompt"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_sampling_compact_runs_after_resume_and_switch_to_smaller_model() {
    skip_if_no_network!();

    let server = MockServer::start().await;
    let previous_model = "gpt-5.3-codex";
    let next_model = "gpt-5.2";

    let model_catalog = ModelsResponse {
        models: vec![
            model_info_with_context_window(previous_model, /*context_window*/ 273_000),
            model_info_with_context_window(next_model, /*context_window*/ 125_000),
        ],
    };
    let resumed_model_catalog = ModelsResponse {
        models: vec![
            model_info_with_context_window(previous_model, /*context_window*/ 273_000),
            model_info_with_context_window(next_model, /*context_window*/ 125_000),
        ],
    };

    let request_log = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_assistant_message("m1", "before resume"),
                ev_completed_with_tokens("r1", /*total_tokens*/ 200_000),
            ]),
            sse(vec![
                ev_assistant_message("m2", "PRE_SAMPLING_SUMMARY"),
                ev_completed_with_tokens("r2", /*total_tokens*/ 10),
            ]),
            sse(vec![
                ev_assistant_message("m3", "after resume"),
                ev_completed_with_tokens("r3", /*total_tokens*/ 100),
            ]),
        ],
    )
    .await;

    let model_provider = responses_mock_model_provider(&server);
    let mut initial_builder = test_codex()
        .with_auth(CodexAuth::create_dummy_api_key_auth_for_testing())
        .with_model(previous_model)
        .with_config(move |config| {
            config.model_provider = model_provider;
            config.model_catalog = Some(model_catalog);
            set_test_compact_prompt(config);
        });
    let initial = initial_builder
        .build(&server)
        .await
        .expect("build initial test codex");
    let home = initial.home.clone();
    let rollout_path = initial
        .session_configured
        .rollout_path
        .clone()
        .expect("rollout path");

    initial
        .codex
        .submit(disabled_permission_user_turn(
            "before resume",
            initial.cwd.path().to_path_buf(),
            previous_model.to_string(),
        ))
        .await
        .expect("submit pre-resume turn");
    wait_for_event(&initial.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    initial
        .codex
        .submit(Op::Shutdown)
        .await
        .expect("shutdown initial session");
    wait_for_event(&initial.codex, |event| {
        matches!(event, EventMsg::ShutdownComplete)
    })
    .await;

    let model_provider = responses_mock_model_provider(&server);
    let mut resumed_builder = test_codex()
        .with_auth(CodexAuth::create_dummy_api_key_auth_for_testing())
        .with_model(previous_model)
        .with_config(move |config| {
            config.model_provider = model_provider;
            config.model_catalog = Some(resumed_model_catalog);
            set_test_compact_prompt(config);
        });
    let resumed = resumed_builder
        .resume(&server, home, rollout_path)
        .await
        .expect("resume codex");

    resumed
        .codex
        .submit(disabled_permission_user_turn(
            "after resume",
            resumed.cwd.path().to_path_buf(),
            next_model.to_string(),
        ))
        .await
        .expect("submit resumed user turn");
    wait_for_event(&resumed.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let requests = wait_for_request_count(&request_log, 3).await;
    assert_eq!(
        requests.len(),
        3,
        "expected user, compact, and follow-up requests"
    );
    assert_pre_sampling_switch_compaction_requests(
        &requests[0].body_json(),
        &requests[1].body_json(),
        &requests[2].body_json(),
        previous_model,
        next_model,
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_compact_persists_rollout_entries() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    let sse1 = sse(vec![
        ev_assistant_message("m1", FIRST_REPLY),
        ev_completed_with_tokens("r1", /*total_tokens*/ 70_000),
    ]);

    let sse2 = sse(vec![
        ev_assistant_message("m2", "SECOND_REPLY"),
        ev_completed_with_tokens("r2", /*total_tokens*/ 330_000),
    ]);

    let auto_summary_payload = auto_summary(AUTO_SUMMARY_TEXT);
    let sse3 = sse(vec![
        ev_assistant_message("m3", &auto_summary_payload),
        ev_completed_with_tokens("r3", /*total_tokens*/ 200),
    ]);
    let sse4 = sse(vec![
        ev_assistant_message("m4", FINAL_REPLY),
        ev_completed_with_tokens("r4", /*total_tokens*/ 120),
    ]);

    let first_matcher = |req: &wiremock::Request| {
        let body = std::str::from_utf8(&req.body).unwrap_or("");
        body.contains(FIRST_AUTO_MSG)
            && !body.contains(SECOND_AUTO_MSG)
            && !body_contains_text(body, SUMMARIZATION_PROMPT)
    };
    mount_sse_once_match(&server, first_matcher, sse1).await;

    let second_matcher = |req: &wiremock::Request| {
        let body = std::str::from_utf8(&req.body).unwrap_or("");
        body.contains(SECOND_AUTO_MSG)
            && body.contains(FIRST_AUTO_MSG)
            && !body_contains_text(body, SUMMARIZATION_PROMPT)
    };
    mount_sse_once_match(&server, second_matcher, sse2).await;

    let third_matcher = |req: &wiremock::Request| {
        let body = std::str::from_utf8(&req.body).unwrap_or("");
        body_contains_text(body, SUMMARIZATION_PROMPT)
    };
    mount_sse_once_match(&server, third_matcher, sse3).await;

    let fourth_matcher = |req: &wiremock::Request| {
        let body = std::str::from_utf8(&req.body).unwrap_or("");
        body.contains(POST_AUTO_USER_MSG) && !body_contains_text(body, SUMMARIZATION_PROMPT)
    };
    mount_sse_once_match(&server, fourth_matcher, sse4).await;

    let model_provider = responses_mock_model_provider(&server);

    let mut builder = test_codex().with_config(move |config| {
        config.model_provider = model_provider;
        set_test_compact_prompt(config);
        config.model_auto_compact_token_limit = Some(200_000);
    });
    let test = builder.build(&server).await.unwrap();
    let codex = test.codex.clone();
    let session_configured = test.session_configured;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: FIRST_AUTO_MSG.into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .unwrap();
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: SECOND_AUTO_MSG.into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .unwrap();
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: POST_AUTO_USER_MSG.into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .unwrap();
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex.submit(Op::Shutdown).await.unwrap();
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::ShutdownComplete)).await;

    let rollout_path = session_configured.rollout_path.expect("rollout path");
    let text = std::fs::read_to_string(&rollout_path).unwrap_or_else(|e| {
        panic!(
            "failed to read rollout file {}: {e}",
            rollout_path.display()
        )
    });

    let mut turn_context_count = 0usize;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(entry): Result<RolloutLine, _> = serde_json::from_str(trimmed) else {
            continue;
        };
        match entry.item {
            RolloutItem::TurnContext(_) => {
                turn_context_count += 1;
            }
            RolloutItem::Compacted(_) => {}
            _ => {}
        }
    }

    assert_eq!(
        turn_context_count, 3,
        "rollout should contain one TurnContext entry per real user turn"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manual_compact_retries_after_context_window_error() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    let user_turn = sse(vec![
        ev_assistant_message("m1", FIRST_REPLY),
        ev_completed("r1"),
    ]);
    let compact_failed = sse_failed(
        "resp-fail",
        "context_length_exceeded",
        CONTEXT_LIMIT_MESSAGE,
    );
    let compact_succeeds = sse(vec![
        ev_assistant_message("m2", SUMMARY_TEXT),
        ev_completed("r2"),
    ]);

    let request_log = mount_sse_sequence(
        &server,
        vec![
            user_turn.clone(),
            compact_failed.clone(),
            compact_succeeds.clone(),
        ],
    )
    .await;

    let model_provider = responses_mock_model_provider(&server);

    let mut builder = test_codex().with_config(move |config| {
        config.model_provider = model_provider;
        set_test_compact_prompt(config);
        config.model_auto_compact_token_limit = Some(200_000);
    });
    let codex = builder.build(&server).await.unwrap().codex;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "first turn".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .unwrap();
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex.submit(Op::Compact).await.unwrap();
    let warning_event = wait_for_event(&codex, |ev| matches!(ev, EventMsg::Warning(_))).await;
    let EventMsg::Warning(WarningEvent { message }) = warning_event else {
        panic!("expected warning event after compact retry");
    };
    assert_eq!(message, COMPACT_WARNING_MESSAGE);
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let requests = request_log.requests();
    assert_eq!(
        requests.len(),
        3,
        "expected user turn and two compact attempts"
    );

    let compact_attempt = requests[1].body_json();
    let retry_attempt = requests[2].body_json();

    let compact_input = compact_attempt["input"]
        .as_array()
        .unwrap_or_else(|| panic!("compact attempt missing input array: {compact_attempt}"));
    let retry_input = retry_attempt["input"]
        .as_array()
        .unwrap_or_else(|| panic!("retry attempt missing input array: {retry_attempt}"));
    let compact_contains_prompt =
        body_contains_text(&compact_attempt.to_string(), SUMMARIZATION_PROMPT);
    let retry_contains_prompt =
        body_contains_text(&retry_attempt.to_string(), SUMMARIZATION_PROMPT);
    assert_eq!(
        compact_contains_prompt, retry_contains_prompt,
        "compact attempts should consistently include or omit the summarization prompt"
    );
    assert!(
        retry_input.len() < compact_input.len(),
        "retry should drop the oldest history item (before {} vs after {})",
        compact_input.len(),
        retry_input.len()
    );
    if let (Some(first_before), Some(first_after)) = (compact_input.first(), retry_input.first()) {
        assert_ne!(
            first_before, first_after,
            "retry should drop the oldest conversation item"
        );
    } else {
        panic!("expected non-empty compact inputs");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// TODO(ccunningham): Re-enable after the follow-up compaction behavior PR lands.
// Current main behavior around non-context manual /compact failures is known-incorrect.
#[ignore = "behavior change covered in follow-up compaction PR"]
async fn manual_compact_non_context_failure_retries_then_emits_task_error() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    let user_turn = sse(vec![
        ev_assistant_message("m1", FIRST_REPLY),
        ev_completed("r1"),
    ]);
    let compact_failed_1 = sse_failed(
        "resp-fail-1",
        "server_error",
        "temporary compact failure one",
    );
    let compact_failed_2 = sse_failed(
        "resp-fail-2",
        "server_error",
        "temporary compact failure two",
    );

    mount_sse_sequence(&server, vec![user_turn, compact_failed_1, compact_failed_2]).await;

    let mut model_provider = responses_mock_model_provider(&server);
    model_provider.stream_max_retries = Some(1);

    let codex = test_codex()
        .with_config(move |config| {
            config.model_provider = model_provider;
            set_test_compact_prompt(config);
            config.model_auto_compact_token_limit = Some(200_000);
        })
        .build(&server)
        .await
        .expect("build codex")
        .codex;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "first turn".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .expect("submit user input");
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex.submit(Op::Compact).await.expect("trigger compact");

    let reconnect_message = wait_for_event_match(&codex, |event| match event {
        EventMsg::StreamError(stream_error) => Some(stream_error.message.clone()),
        _ => None,
    })
    .await;
    assert!(
        reconnect_message.contains("Reconnecting... 1/1"),
        "expected reconnect stream error message, got {reconnect_message}"
    );

    let task_error_message = wait_for_event_match(&codex, |event| match event {
        EventMsg::Error(err) => Some(err.message.clone()),
        _ => None,
    })
    .await;
    assert!(
        task_error_message.contains("Error running local compact task"),
        "expected local compact task error prefix, got {task_error_message}"
    );
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manual_compact_twice_preserves_latest_user_messages() {
    skip_if_no_network!();

    let first_user_message = "first manual turn";
    let second_user_message = "second manual turn";
    let final_user_message = "post compact follow-up";
    let first_summary = "FIRST_MANUAL_SUMMARY";
    let second_summary = "SECOND_MANUAL_SUMMARY";
    let expected_second_summary = summary_with_prefix(second_summary);

    let server = start_mock_server().await;

    let first_turn = sse(vec![
        ev_assistant_message("m1", FIRST_REPLY),
        ev_completed("r1"),
    ]);
    let first_compact_summary = auto_summary(first_summary);
    let first_compact = sse(vec![
        ev_assistant_message("m2", &first_compact_summary),
        ev_completed("r2"),
    ]);
    let second_turn = sse(vec![
        ev_assistant_message("m3", SECOND_LARGE_REPLY),
        ev_completed("r3"),
    ]);
    let second_compact_summary = auto_summary(second_summary);
    let second_compact = sse(vec![
        ev_assistant_message("m4", &second_compact_summary),
        ev_completed("r4"),
    ]);
    let final_turn = sse(vec![
        ev_assistant_message("m5", FINAL_REPLY),
        ev_completed("r5"),
    ]);

    let responses_mock = mount_sse_sequence(
        &server,
        vec![
            first_turn,
            first_compact,
            second_turn,
            second_compact,
            final_turn,
        ],
    )
    .await;

    let model_provider = responses_mock_model_provider(&server);

    let mut builder = test_codex().with_config(move |config| {
        config.model_provider = model_provider;
        set_test_compact_prompt(config);
    });
    let codex = builder.build(&server).await.unwrap().codex;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: first_user_message.into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .unwrap();
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex.submit(Op::Compact).await.unwrap();
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: second_user_message.into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .unwrap();
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex.submit(Op::Compact).await.unwrap();
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: final_user_message.into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .unwrap();
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let requests = responses_mock.requests();
    assert_eq!(
        requests.len(),
        5,
        "expected exactly 5 requests (user turn, compact, user turn, compact, final turn)"
    );
    let contains_user_text = |request: &core_test_support::responses::ResponsesRequest,
                              expected: &str| {
        request
            .message_input_texts("user")
            .iter()
            .any(|text| text == expected)
    };

    assert!(
        contains_user_text(&requests[0], first_user_message),
        "first turn request missing first user message"
    );
    assert!(
        !contains_user_text(&requests[0], SUMMARIZATION_PROMPT),
        "first turn request should not include summarization prompt"
    );

    assert!(
        contains_user_text(&requests[1], first_user_message),
        "first compact request should include history before compaction"
    );
    let compact_metadata: Value = serde_json::from_str(
        &requests[1]
            .header("x-astral-turn-metadata")
            .expect("local compact request should include turn metadata"),
    )
    .expect("local compact turn metadata should be valid json");
    assert_eq!(
        compact_metadata["request_kind"].as_str(),
        Some("compaction")
    );
    assert_eq!(
        compact_metadata["window_id"].as_str(),
        requests[1].header("x-astral-window-id").as_deref()
    );
    assert_eq!(
        compact_metadata["compaction"],
        json!({
            "trigger": "manual",
            "reason": "user_requested",
            "implementation": "local_model",
            "phase": "standalone_turn",
            "strategy": "memento",
        })
    );

    assert!(
        contains_user_text(&requests[2], second_user_message),
        "second turn request missing second user message"
    );
    let next_turn_metadata: Value = serde_json::from_str(
        &requests[2]
            .header("x-astral-turn-metadata")
            .expect("next regular request should include turn metadata"),
    )
    .expect("next regular turn metadata should be valid json");
    assert_eq!(
        next_turn_metadata["request_kind"].as_str(),
        Some("turn"),
        "regular requests after compaction should remain turn requests"
    );
    assert_eq!(
        next_turn_metadata["window_id"].as_str(),
        requests[2].header("x-astral-window-id").as_deref()
    );
    assert_ne!(
        compact_metadata["window_id"], next_turn_metadata["window_id"],
        "the next request should use the new compacted context window"
    );
    assert!(
        next_turn_metadata.get("compaction").is_none(),
        "regular requests after compaction should not be marked as compact requests"
    );
    assert!(
        contains_user_text(&requests[2], first_user_message),
        "second turn request should include the compacted user history"
    );

    assert!(
        contains_user_text(&requests[3], second_user_message),
        "second compact request should include latest history"
    );

    insta::assert_snapshot!(
        "manual_compact_with_history_shapes",
        format_labeled_requests_snapshot(
            "Manual /compact with prior user history compacts existing history and the follow-up turn includes the compact summary plus new user message.",
            &[
                ("Local Compaction Request", &requests[1]),
                ("Local Post-Compaction History Layout", &requests[2]),
            ]
        )
    );

    let first_compact_has_prompt = contains_user_text(&requests[1], SUMMARIZATION_PROMPT);
    let second_compact_has_prompt = contains_user_text(&requests[3], SUMMARIZATION_PROMPT);
    assert_eq!(
        first_compact_has_prompt, second_compact_has_prompt,
        "compact requests should consistently include or omit the summarization prompt"
    );

    let first_request_user_texts = requests[0].message_input_texts("user");
    let first_turn_user_index = first_request_user_texts
        .len()
        .checked_sub(1)
        .unwrap_or_else(|| panic!("first turn request missing user messages"));
    assert_eq!(
        first_request_user_texts[first_turn_user_index], first_user_message,
        "first turn request should end with the submitted user message"
    );
    let initial_seeded_user_prefix = &first_request_user_texts[..first_turn_user_index];

    let final_request_user_texts = requests
        .last()
        .unwrap_or_else(|| panic!("final turn request missing for {final_user_message}"))
        .message_input_texts("user");
    assert!(
        !initial_seeded_user_prefix.is_empty(),
        "first turn should include seeded user prefix before the submitted user message"
    );
    let (final_request_last_user_text, final_request_before_last_user) = final_request_user_texts
        .split_last()
        .unwrap_or_else(|| panic!("final turn request missing user messages"));
    assert_eq!(
        final_request_last_user_text, final_user_message,
        "final turn request should end with the submitted user message"
    );
    let history_before_seeded_prefix = final_request_before_last_user
        .strip_suffix(initial_seeded_user_prefix)
        .unwrap_or_else(|| {
            panic!(
                "final request should end with the seeded user prefix from the first request: {initial_seeded_user_prefix:?}"
            )
        });
    let expected_history = vec![
        first_user_message.to_string(),
        second_user_message.to_string(),
        expected_second_summary,
    ];
    assert_eq!(history_before_seeded_prefix, expected_history.as_slice());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_compact_allows_multiple_attempts_when_interleaved_with_other_turn_events() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    let sse1 = sse(vec![
        ev_assistant_message("m1", FIRST_REPLY),
        ev_completed_with_tokens("r1", /*total_tokens*/ 500),
    ]);
    let first_summary_payload = auto_summary(FIRST_AUTO_SUMMARY);
    let sse2 = sse(vec![
        ev_assistant_message("m2", &first_summary_payload),
        ev_completed_with_tokens("r2", /*total_tokens*/ 50),
    ]);
    let sse3 = sse(vec![
        ev_function_call(DUMMY_CALL_ID, DUMMY_FUNCTION_NAME, "{}"),
        ev_completed_with_tokens("r3", /*total_tokens*/ 150),
    ]);
    let sse4 = sse(vec![
        ev_assistant_message("m4", SECOND_LARGE_REPLY),
        ev_completed_with_tokens("r4", /*total_tokens*/ 450),
    ]);
    let second_summary_payload = auto_summary(SECOND_AUTO_SUMMARY);
    let sse5 = sse(vec![
        ev_assistant_message("m5", &second_summary_payload),
        ev_completed_with_tokens("r5", /*total_tokens*/ 60),
    ]);
    let sse6 = sse(vec![
        ev_assistant_message("m6", FINAL_REPLY),
        ev_completed_with_tokens("r6", /*total_tokens*/ 120),
    ]);
    let follow_up_user = "FOLLOW_UP_AUTO_COMPACT";
    let final_user = "FINAL_AUTO_COMPACT";

    let request_log = mount_sse_sequence(&server, vec![sse1, sse2, sse3, sse4, sse5, sse6]).await;

    let model_provider = responses_mock_model_provider(&server);

    let mut builder = test_codex().with_config(move |config| {
        config.model_provider = model_provider;
        set_test_compact_prompt(config);
        config.model_auto_compact_token_limit = Some(200);
    });
    let codex = builder.build(&server).await.unwrap().codex;

    let mut auto_compact_lifecycle_events = Vec::new();
    for user in [MULTI_AUTO_MSG, follow_up_user, final_user] {
        codex
            .submit(Op::UserInput {
                items: vec![UserInput::Text {
                    text: user.into(),
                    text_elements: Vec::new(),
                }],
                final_output_json_schema: None,
                model_client_metadata: None,
                additional_context: Default::default(),
                thread_settings: Default::default(),
            })
            .await
            .unwrap();

        loop {
            let event = codex.next_event().await.unwrap();
            if event.id.starts_with("auto-compact-")
                && matches!(
                    event.msg,
                    EventMsg::TurnStarted(_) | EventMsg::TurnComplete(_)
                )
            {
                auto_compact_lifecycle_events.push(event);
                continue;
            }
            if let EventMsg::TurnComplete(_) = &event.msg
                && !event.id.starts_with("auto-compact-")
            {
                break;
            }
        }
    }

    assert!(
        auto_compact_lifecycle_events.is_empty(),
        "auto compact should not emit task lifecycle events"
    );

    let request_bodies: Vec<String> = request_log
        .requests()
        .into_iter()
        .map(|request| request.body_json().to_string())
        .collect();
    assert_eq!(
        request_bodies.len(),
        6,
        "expected six requests including two auto compactions"
    );
    assert!(
        request_bodies[0].contains(MULTI_AUTO_MSG),
        "first request should contain the user input"
    );
    assert!(
        body_contains_text(&request_bodies[1], SUMMARIZATION_PROMPT),
        "first auto compact request should include the summarization prompt"
    );
    assert!(
        request_bodies[3].contains(&format!("unsupported call: {DUMMY_FUNCTION_NAME}")),
        "function call output should be sent before the second auto compact"
    );
    assert!(
        body_contains_text(&request_bodies[4], SUMMARIZATION_PROMPT),
        "second auto compact request should include the summarization prompt"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn snapshot_request_shape_mid_turn_continuation_compaction() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    let context_window = 100;
    let limit = context_window * 90 / 100;
    let over_limit_tokens = context_window * 95 / 100 + 1;

    let first_turn = sse(vec![
        ev_function_call(DUMMY_CALL_ID, DUMMY_FUNCTION_NAME, "{}"),
        ev_completed_with_tokens("r1", over_limit_tokens),
    ]);
    let auto_summary_payload = auto_summary(AUTO_SUMMARY_TEXT);
    let auto_compact_turn = sse(vec![
        ev_assistant_message("m2", &auto_summary_payload),
        ev_completed_with_tokens("r3", /*total_tokens*/ 10),
    ]);
    let post_auto_compact_turn = sse(vec![
        ev_assistant_message("m3", FINAL_REPLY),
        ev_completed_with_tokens("r4", /*total_tokens*/ 10),
    ]);

    // Mount responses in order and keep mocks only for the ones we assert on.
    let first_turn_mock = mount_sse_once(&server, first_turn).await;
    let auto_compact_mock = mount_sse_once(&server, auto_compact_turn).await;
    let post_auto_compact_mock = mount_sse_once(&server, post_auto_compact_turn).await;

    let model_provider = responses_mock_model_provider(&server);

    let mut builder = test_codex().with_config(move |config| {
        config.model_provider = model_provider;
        set_test_compact_prompt(config);
        config.model_context_window = Some(context_window);
        config.model_auto_compact_token_limit = Some(limit);
    });
    let codex = builder.build(&server).await.unwrap().codex;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: FUNCTION_CALL_LIMIT_MSG.into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .unwrap();

    wait_for_event(&codex, |msg| matches!(msg, EventMsg::TurnComplete(_))).await;

    // Assert first request captured expected user message that triggers function call.
    let first_request = first_turn_mock.single_request().input();
    assert!(
        first_request.iter().any(|item| {
            item.get("type").and_then(|value| value.as_str()) == Some("message")
                && item
                    .get("content")
                    .and_then(|content| content.as_array())
                    .and_then(|entries| entries.first())
                    .and_then(|entry| entry.get("text"))
                    .and_then(|value| value.as_str())
                    == Some(FUNCTION_CALL_LIMIT_MSG)
        }),
        "first request should include the user message that triggers the function call"
    );

    let function_call_output = auto_compact_mock
        .single_request()
        .function_call_output(DUMMY_CALL_ID);
    let output_text = function_call_output
        .get("output")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    assert!(
        output_text.contains(DUMMY_FUNCTION_NAME),
        "function call output should be sent before auto compact"
    );

    let auto_compact_body = auto_compact_mock.single_request().body_json().to_string();
    assert!(
        body_contains_text(&auto_compact_body, SUMMARIZATION_PROMPT),
        "mid-turn auto compact request should include the summarization prompt after exceeding 95% (limit {limit})"
    );

    insta::assert_snapshot!(
        "mid_turn_compaction_shapes",
        format_labeled_requests_snapshot(
            "True mid-turn continuation compaction after tool output: compact request includes tool artifacts, and the continuation request includes the summary in the same turn.",
            &[
                (
                    "Local Compaction Request",
                    &auto_compact_mock.single_request()
                ),
                (
                    "Local Post-Compaction History Layout",
                    &post_auto_compact_mock.single_request()
                ),
            ]
        )
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_compact_clamps_config_limit_to_context_window() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    let context_window = 100;
    let config_limit = 200;
    let over_limit_tokens = context_window + 1;

    let first_turn = sse(vec![
        ev_assistant_message("m1", FIRST_REPLY),
        ev_completed_with_tokens("r1", over_limit_tokens),
    ]);
    let auto_summary_payload = auto_summary(AUTO_SUMMARY_TEXT);
    let auto_compact_turn = sse(vec![
        ev_assistant_message("m2", &auto_summary_payload),
        ev_completed_with_tokens("r2", /*total_tokens*/ 10),
    ]);
    let post_auto_compact_turn = sse(vec![ev_completed_with_tokens(
        "r3", /*total_tokens*/ 10,
    )]);

    let first_turn_mock = mount_sse_once(&server, first_turn).await;
    let auto_compact_mock = mount_sse_once(&server, auto_compact_turn).await;
    mount_sse_once(&server, post_auto_compact_turn).await;

    let model_provider = responses_mock_model_provider(&server);
    let mut builder = test_codex().with_config(move |config| {
        config.model_provider = model_provider;
        set_test_compact_prompt(config);
        config.model_context_window = Some(context_window);
        config.model_auto_compact_token_limit = Some(config_limit);
    });
    let codex = builder.build(&server).await.unwrap();

    codex.submit_turn("OVER_LIMIT_TURN").await.unwrap();
    codex.submit_turn("FOLLOW_UP_AFTER_CLAMP").await.unwrap();
    let auto_compact_request = wait_for_request_count(&auto_compact_mock, 1)
        .await
        .remove(0);

    assert!(
        first_turn_mock.single_request().input().iter().any(|item| {
            item.get("type").and_then(|value| value.as_str()) == Some("message")
                && item
                    .get("content")
                    .and_then(|content| content.as_array())
                    .and_then(|entries| entries.first())
                    .and_then(|entry| entry.get("text"))
                    .and_then(|value| value.as_str())
                    == Some("OVER_LIMIT_TURN")
        }),
        "first request should contain the over-limit user input"
    );

    let auto_compact_body = auto_compact_request.body_json().to_string();
    assert!(
        body_contains_text(&auto_compact_body, SUMMARIZATION_PROMPT),
        "auto compact should run with the summarization prompt when config limit exceeds context"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_compact_body_after_prefix_ignores_starting_window_prefix() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    let first_turn = sse(vec![
        ev_assistant_message("m1", FIRST_REPLY),
        ev_completed_with_usage("r1", /*input_tokens*/ 600, /*output_tokens*/ 50),
    ]);
    let second_turn = sse(vec![
        ev_assistant_message("m2", SECOND_LARGE_REPLY),
        ev_completed_with_usage("r2", /*input_tokens*/ 700, /*output_tokens*/ 50),
    ]);
    let auto_compact_turn = sse(vec![
        ev_assistant_message("m3", AUTO_SUMMARY_TEXT),
        ev_completed_with_tokens("r3", /*total_tokens*/ 20),
    ]);
    let third_turn = sse(vec![
        ev_assistant_message("m4", FINAL_REPLY),
        ev_completed_with_usage("r4", /*input_tokens*/ 750, /*output_tokens*/ 20),
    ]);
    let request_log = mount_sse_sequence(
        &server,
        vec![first_turn, second_turn, auto_compact_turn, third_turn],
    )
    .await;

    let model_provider = responses_mock_model_provider(&server);
    let test = test_codex()
        .with_config(move |config| {
            config.model_provider = model_provider;
            set_test_compact_prompt(config);
            config.model_context_window = Some(1_000);
            config.model_auto_compact_token_limit = Some(100);
            config.model_auto_compact_token_limit_scope =
                AutoCompactTokenLimitScope::BodyAfterPrefix;
        })
        .build(&server)
        .await
        .expect("build codex");

    for user in ["PREFIX_FREE_ONE", "PREFIX_FREE_TWO"] {
        test.submit_turn(user).await.expect("submit turn");
    }

    assert_eq!(
        request_log.requests().len(),
        2,
        "the first two turns should not compact just because the prefix exceeds the body budget"
    );

    test.submit_turn("PREFIX_FREE_THREE")
        .await
        .expect("submit third turn");

    let requests = wait_for_request_count(&request_log, 3).await;
    assert_eq!(
        requests.len(),
        4,
        "third turn should include pre-turn compaction plus the post-compaction request"
    );
    let compact_body = requests[2].body_json().to_string();
    assert!(
        body_contains_text(&compact_body, SUMMARIZATION_PROMPT),
        "body-after-prefix mode should compact once tokens after the first assistant sample exceed the configured budget"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_compact_body_after_prefix_counts_growth_after_compaction() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    let first_turn = sse(vec![
        ev_assistant_message("m1", FIRST_REPLY),
        ev_completed_with_usage("r1", /*input_tokens*/ 100, /*output_tokens*/ 50),
    ]);
    let first_auto_compact_turn = sse(vec![
        ev_assistant_message("m2", AUTO_SUMMARY_TEXT),
        ev_completed_with_tokens("r2", /*total_tokens*/ 20),
    ]);
    let second_turn = sse(vec![
        ev_assistant_message("m3", SECOND_LARGE_REPLY),
        ev_completed_with_usage(
            "r3", /*input_tokens*/ 100_000, /*output_tokens*/ 10,
        ),
    ]);
    let third_turn = sse(vec![
        ev_assistant_message("m4", FINAL_REPLY),
        ev_completed_with_usage(
            "r4", /*input_tokens*/ 100_100, /*output_tokens*/ 5,
        ),
    ]);
    let second_auto_compact_turn = sse(vec![
        ev_assistant_message("m5", AUTO_SUMMARY_TEXT),
        ev_completed_with_tokens("r5", /*total_tokens*/ 20),
    ]);
    let fourth_turn = sse(vec![
        ev_assistant_message("m6", FINAL_REPLY),
        ev_completed_with_usage("r6", /*input_tokens*/ 80, /*output_tokens*/ 5),
    ]);
    let request_log = mount_sse_sequence(
        &server,
        vec![
            first_turn,
            first_auto_compact_turn,
            second_turn,
            third_turn,
            second_auto_compact_turn,
            fourth_turn,
        ],
    )
    .await;

    let model_provider = responses_mock_model_provider(&server);
    let test = test_codex()
        .with_config(move |config| {
            config.model_provider = model_provider;
            set_test_compact_prompt(config);
            config.model_context_window = Some(200_000);
            config.model_auto_compact_token_limit = Some(40);
            config.model_auto_compact_token_limit_scope =
                AutoCompactTokenLimitScope::BodyAfterPrefix;
        })
        .build(&server)
        .await
        .expect("build codex");

    test.submit_turn("WINDOW_PREFIX")
        .await
        .expect("submit first turn");
    test.submit_turn("GROWTH_AFTER_COMPACT")
        .await
        .expect("submit second turn");

    let requests = wait_for_request_count(&request_log, 3).await;
    assert_eq!(
        requests.len(),
        3,
        "second turn should compact first and then sample the new growth"
    );

    test.submit_turn("AFTER_GROWTH")
        .await
        .expect("submit third turn");

    let requests = wait_for_request_count(&request_log, 4).await;
    assert_eq!(
        requests.len(),
        4,
        "the first server-observed input in the new window should become the prefill baseline"
    );

    test.submit_turn("AFTER_GROWTH_TRIGGER")
        .await
        .expect("submit fourth turn");

    let requests = wait_for_request_count(&request_log, 6).await;
    assert_eq!(
        requests.len(),
        6,
        "fourth turn should compact because later post-compaction growth counted against the body budget"
    );
    let compact_body = requests[4].body_json().to_string();
    assert!(
        body_contains_text(&compact_body, SUMMARIZATION_PROMPT),
        "post-compaction growth should trigger a second body-after-prefix compaction"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_compact_body_after_prefix_still_caps_at_context_window() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    let first_turn = sse(vec![
        ev_assistant_message("m1", FIRST_REPLY),
        ev_completed_with_usage("r1", /*input_tokens*/ 80, /*output_tokens*/ 5),
    ]);
    let second_turn = sse(vec![
        ev_assistant_message("m2", SECOND_LARGE_REPLY),
        ev_completed_with_usage("r2", /*input_tokens*/ 98, /*output_tokens*/ 1),
    ]);
    let auto_compact_turn = sse(vec![
        ev_assistant_message("m3", AUTO_SUMMARY_TEXT),
        ev_completed_with_tokens("r3", /*total_tokens*/ 20),
    ]);
    let third_turn = sse(vec![
        ev_assistant_message("m4", FINAL_REPLY),
        ev_completed_with_usage("r4", /*input_tokens*/ 80, /*output_tokens*/ 5),
    ]);
    let request_log = mount_sse_sequence(
        &server,
        vec![first_turn, second_turn, auto_compact_turn, third_turn],
    )
    .await;

    let model_provider = responses_mock_model_provider(&server);
    let test = test_codex()
        .with_config(move |config| {
            config.model_provider = model_provider;
            set_test_compact_prompt(config);
            config.model_context_window = Some(100);
            config.model_auto_compact_token_limit = Some(200);
            config.model_auto_compact_token_limit_scope =
                AutoCompactTokenLimitScope::BodyAfterPrefix;
        })
        .build(&server)
        .await
        .expect("build codex");

    for user in ["CONTEXT_CAP_ONE", "CONTEXT_CAP_TWO", "CONTEXT_CAP_THREE"] {
        test.submit_turn(user).await.expect("submit turn");
    }

    let requests = wait_for_request_count(&request_log, 4).await;
    assert_eq!(
        requests.len(),
        4,
        "third turn should compact before sampling because total context hit the usable window"
    );
    let compact_body = requests[2].body_json().to_string();
    assert!(
        body_contains_text(&compact_body, SUMMARIZATION_PROMPT),
        "body-after-prefix mode should still clamp the total threshold to the usable context window"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_compact_runs_when_prior_usage_exceeds_limit_before_last_user() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    let first_user = "COUNT_PRE_LAST_REASONING";
    let second_user = "TRIGGER_COMPACT_AT_LIMIT";
    let third_user = "AFTER_LOCAL_COMPACT";
    let local_summary = "LOCAL_COMPACT_SUMMARY";

    let pre_last_reasoning_content = "a".repeat(2_400);
    let post_last_reasoning_content = "b".repeat(4_000);

    let first_turn = sse(vec![
        ev_reasoning_item("pre-reasoning", &["pre"], &[&pre_last_reasoning_content]),
        ev_completed_with_tokens("r1", /*total_tokens*/ 310),
    ]);
    let second_turn = sse(vec![
        ev_reasoning_item("post-reasoning", &["post"], &[&post_last_reasoning_content]),
        ev_completed_with_tokens("r2", /*total_tokens*/ 80),
    ]);
    let auto_compact_turn = sse(vec![
        ev_assistant_message("compact-summary", local_summary),
        ev_completed_with_tokens("r3", /*total_tokens*/ 20),
    ]);
    let third_turn = sse(vec![
        ev_assistant_message("m4", FINAL_REPLY),
        ev_completed_with_tokens("r4", /*total_tokens*/ 1),
    ]);

    let request_log = mount_sse_sequence(
        &server,
        vec![
            // Turn 1: reasoning before last user (should count).
            first_turn,
            // Turn 2: reasoning after last user (should be ignored for compaction).
            second_turn,
            // Local compact runs before turn 3.
            auto_compact_turn,
            // Turn 3: next user turn after local compaction.
            third_turn,
        ],
    )
    .await;
    let hosted_base_url = format!("{}/backend-api", server.uri());

    let codex = test_codex()
        .with_auth(CodexAuth::create_dummy_api_key_auth_for_testing())
        .with_config(move |config| {
            config.hosted_base_url = hosted_base_url;
            set_test_compact_prompt(config);
            config.model_auto_compact_token_limit = Some(300);
        })
        .build(&server)
        .await
        .expect("build codex")
        .codex;

    for user in [first_user, second_user, third_user] {
        codex
            .submit(Op::UserInput {
                items: vec![UserInput::Text {
                    text: user.into(),
                    text_elements: Vec::new(),
                }],
                final_output_json_schema: None,
                model_client_metadata: None,
                additional_context: Default::default(),
                thread_settings: Default::default(),
            })
            .await
            .unwrap();
        wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;
    }

    let requests = wait_for_request_count(&request_log, 4).await;
    assert_eq!(
        requests.len(),
        4,
        "conversation should include three user turns and one local compact turn"
    );
    let compact_index = requests
        .iter()
        .position(|request| {
            body_contains_text(&request.body_json().to_string(), SUMMARIZATION_PROMPT)
        })
        .unwrap_or_else(|| panic!("expected one local compact request"));
    let compact_body = requests[compact_index].body_json().to_string();
    assert!(
        body_contains_text(&compact_body, SUMMARIZATION_PROMPT),
        "conversation should include the local compact request"
    );
    let post_compact_has_history = requests.iter().skip(compact_index + 1).any(|request| {
        let body = request.body_json().to_string();
        body.contains(local_summary) || body.contains(FINAL_REPLY)
    });
    assert!(
        post_compact_has_history,
        "a post-compact user turn should include compacted history"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_compact_runs_when_reasoning_header_clears_between_turns() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    let first_user = "SERVER_INCLUDED_FIRST";
    let second_user = "SERVER_INCLUDED_SECOND";
    let third_user = "SERVER_INCLUDED_THIRD";

    let pre_last_reasoning_content = "a".repeat(2_400);
    let post_last_reasoning_content = "b".repeat(4_000);

    let first_turn = sse(vec![
        ev_reasoning_item("pre-reasoning", &["pre"], &[&pre_last_reasoning_content]),
        ev_completed_with_tokens("r1", /*total_tokens*/ 10),
    ]);
    let second_turn = sse(vec![
        ev_reasoning_item("post-reasoning", &["post"], &[&post_last_reasoning_content]),
        ev_completed_with_tokens("r2", /*total_tokens*/ 310),
    ]);
    let auto_compact_turn = sse(vec![
        ev_assistant_message("compact-summary", "LOCAL_COMPACT_SUMMARY"),
        ev_completed_with_tokens("r3", /*total_tokens*/ 20),
    ]);
    let third_turn = sse(vec![
        ev_assistant_message("m4", FINAL_REPLY),
        ev_completed_with_tokens("r4", /*total_tokens*/ 1),
    ]);

    let responses = vec![
        sse_response(first_turn).insert_header("X-Reasoning-Included", "true"),
        sse_response(second_turn),
        sse_response(auto_compact_turn),
        sse_response(third_turn),
    ];
    let request_log = mount_response_sequence(&server, responses).await;

    let codex = test_codex()
        .with_auth(CodexAuth::create_dummy_api_key_auth_for_testing())
        .with_config(|config| {
            set_test_compact_prompt(config);
            config.model_auto_compact_token_limit = Some(300);
        })
        .build(&server)
        .await
        .expect("build codex")
        .codex;

    for user in [first_user, second_user, third_user] {
        codex
            .submit(Op::UserInput {
                items: vec![UserInput::Text {
                    text: user.into(),
                    text_elements: Vec::new(),
                }],
                final_output_json_schema: None,
                model_client_metadata: None,
                additional_context: Default::default(),
                thread_settings: Default::default(),
            })
            .await
            .unwrap();
        wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;
    }

    let requests = wait_for_request_count(&request_log, 4).await;
    assert_eq!(
        requests.len(),
        4,
        "local compaction should run once after the reasoning header clears"
    );
    assert!(
        body_contains_text(&requests[2].body_json().to_string(), SUMMARIZATION_PROMPT),
        "third request should be the local compact request"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// TODO(ccunningham): Update once pre-turn compaction includes incoming user input.
async fn snapshot_request_shape_pre_turn_compaction_including_incoming_user_message() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    let sse1 = sse(vec![
        ev_assistant_message("m1", FIRST_REPLY),
        ev_completed_with_tokens("r1", /*total_tokens*/ 60),
    ]);
    let sse2 = sse(vec![
        ev_assistant_message("m2", "SECOND_REPLY"),
        ev_completed_with_tokens("r2", /*total_tokens*/ 500),
    ]);
    let sse3 = sse(vec![
        ev_assistant_message("m3", "PRE_TURN_SUMMARY"),
        ev_completed_with_tokens("r3", /*total_tokens*/ 100),
    ]);
    let sse4 = sse(vec![
        ev_assistant_message("m4", FINAL_REPLY),
        ev_completed_with_tokens("r4", /*total_tokens*/ 80),
    ]);
    let request_log = mount_sse_sequence(&server, vec![sse1, sse2, sse3, sse4]).await;

    let model_provider = responses_mock_model_provider(&server);
    let codex = test_codex()
        .with_config(move |config| {
            config.model_provider = model_provider;
            set_test_compact_prompt(config);
            config.model_auto_compact_token_limit = Some(200);
        })
        .build(&server)
        .await
        .expect("build codex")
        .codex;

    for user in ["USER_ONE", "USER_TWO"] {
        codex
            .submit(Op::UserInput {
                items: vec![UserInput::Text {
                    text: user.to_string(),
                    text_elements: Vec::new(),
                }],
                final_output_json_schema: None,
                model_client_metadata: None,
                additional_context: Default::default(),
                thread_settings: Default::default(),
            })
            .await
            .expect("submit user input");
        wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;
    }
    core_test_support::submit_thread_settings(
        &codex,
        codex_protocol::protocol::ThreadSettingsOverrides {
            environments: Some(local_selections(
                test_path_buf(PRETURN_CONTEXT_DIFF_CWD).abs(),
            )),
            ..Default::default()
        },
    )
    .await
    .expect("override thread settings");
    let image_url = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR4nGNgYAAAAAMAASsJTYQAAAAASUVORK5CYII="
        .to_string();
    codex
        .submit(Op::UserInput {
            items: vec![
                UserInput::Image {
                    image_url: image_url.clone(),
                    detail: None,
                },
                UserInput::Text {
                    text: "USER_THREE".to_string(),
                    text_elements: Vec::new(),
                },
            ],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .expect("submit user input");
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let requests = request_log.requests();
    assert_eq!(requests.len(), 4, "expected user, user, compact, follow-up");

    insta::assert_snapshot!(
        "pre_turn_compaction_including_incoming_shapes",
        format_labeled_requests_snapshot(
            "Pre-turn auto-compaction with a context override emits the context diff in the compact request while the incoming user message is still excluded.",
            &[
                ("Local Compaction Request", &requests[2]),
                ("Local Post-Compaction History Layout", &requests[3]),
            ]
        )
    );
    let compact_request_user_texts = requests[2].message_input_texts("user");
    assert!(
        !compact_request_user_texts
            .iter()
            .any(|text| text == "USER_THREE"),
        "current behavior excludes incoming user message from pre-turn compaction input"
    );
    let follow_up_user_texts = requests[3].message_input_texts("user");
    assert!(
        follow_up_user_texts.iter().any(|text| text == "USER_THREE"),
        "expected post-compaction follow-up request to keep incoming user text"
    );
    let follow_up_user_images = requests[3].message_input_image_urls("user");
    assert!(
        follow_up_user_images
            .iter()
            .any(|url| url == image_url.as_str()),
        "expected post-compaction follow-up request to keep incoming user image content"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// TODO(ccunningham): Update once pre-turn compaction context-overflow handling includes incoming
// user input and emits richer oversized-input messaging.
async fn snapshot_request_shape_pre_turn_compaction_strips_incoming_model_switch() {
    skip_if_no_network!();

    let server = start_mock_server().await;
    let previous_model = "gpt-5.4";
    let next_model = "gpt-5.3-codex";

    let request_log = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_assistant_message("m1", "BEFORE_SWITCH_REPLY"),
                ev_completed_with_tokens("r1", /*total_tokens*/ 500),
            ]),
            sse(vec![
                ev_assistant_message("m2", "PRETURN_SWITCH_SUMMARY"),
                ev_completed_with_tokens("r2", /*total_tokens*/ 100),
            ]),
            sse(vec![
                ev_assistant_message("m3", "AFTER_SWITCH_REPLY"),
                ev_completed_with_tokens("r3", /*total_tokens*/ 100),
            ]),
        ],
    )
    .await;

    let model_provider = responses_mock_model_provider(&server);
    let test = test_codex()
        .with_auth(CodexAuth::create_dummy_api_key_auth_for_testing())
        .with_model(previous_model)
        .with_config(move |config| {
            config.model_provider = model_provider;
            set_test_compact_prompt(config);
            let _ = config.features.enable(Feature::RemoteModels);
            config.model_auto_compact_token_limit = Some(200);
        })
        .build(&server)
        .await
        .expect("build codex");

    test.codex
        .submit(disabled_permission_user_turn(
            "BEFORE_SWITCH_USER",
            test.cwd.path().to_path_buf(),
            previous_model.to_string(),
        ))
        .await
        .expect("submit first user turn");
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    test.codex
        .submit(disabled_permission_user_turn(
            "AFTER_SWITCH_USER",
            test.cwd.path().to_path_buf(),
            next_model.to_string(),
        ))
        .await
        .expect("submit second user turn");
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let requests = request_log.requests();
    assert_eq!(
        requests.len(),
        3,
        "expected first turn, pre-turn compact, and post-compact follow-up requests"
    );

    let compact_body = requests[1].body_json().to_string();
    assert!(
        body_contains_text(&compact_body, SUMMARIZATION_PROMPT),
        "pre-turn compaction request should include summarization prompt"
    );
    assert!(
        !compact_body.contains("<model_switch>"),
        "pre-turn compaction request should strip incoming model-switch update item"
    );

    let follow_up_body = requests[2].body_json().to_string();
    assert!(
        follow_up_body.contains("<model_switch>"),
        "post-compaction follow-up should include model-switch update item"
    );

    insta::assert_snapshot!(
        "pre_turn_compaction_strips_incoming_model_switch_shapes",
        format_labeled_requests_snapshot(
            "Pre-turn compaction during model switch (without pre-sampling model-switch compaction): current behavior strips incoming <model_switch> from the compact request and restores it in the post-compaction follow-up request.",
            &[
                ("Initial Request (Previous Model)", &requests[0]),
                ("Local Compaction Request", &requests[1]),
                ("Local Post-Compaction History Layout", &requests[2]),
            ]
        )
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn snapshot_request_shape_pre_turn_compaction_context_window_exceeded() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    let first_turn = sse(vec![
        ev_assistant_message("m1", FIRST_REPLY),
        ev_completed_with_tokens("r1", /*total_tokens*/ 500),
    ]);
    let mut responses = vec![first_turn];
    responses.extend(
        (0..5).map(|_| {
            sse_failed(
                "compact-failed",
                "context_length_exceeded",
                "Your input exceeds the context window of this model. Please adjust your input and try again.",
            )
        }),
    );
    let request_log = mount_sse_sequence(&server, responses).await;

    let mut model_provider = responses_mock_model_provider(&server);
    model_provider.stream_max_retries = Some(0);
    let codex = test_codex()
        .with_config(move |config| {
            config.model_provider = model_provider;
            set_test_compact_prompt(config);
            config.model_auto_compact_token_limit = Some(200);
        })
        .build(&server)
        .await
        .expect("build codex")
        .codex;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "USER_ONE".to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .expect("submit first user");
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "USER_TWO".to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .expect("submit second user");
    let error_message = wait_for_event_match(&codex, |event| match event {
        EventMsg::Error(err) => Some(err.message.clone()),
        _ => None,
    })
    .await;
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let requests = request_log.requests();
    assert!(
        requests.len() >= 2,
        "expected first turn and at least one compaction request"
    );

    insta::assert_snapshot!(
        "pre_turn_compaction_context_window_exceeded_shapes",
        format_labeled_requests_snapshot(
            "Pre-turn auto-compaction context-window failure: compaction request excludes the incoming user message and the turn errors.",
            &[(
                "Local Compaction Request (Incoming User Excluded)",
                &requests[1]
            ),]
        )
    );

    assert!(
        error_message.contains("ran out of room in the model's context window"),
        "expected context window exceeded message, got {error_message}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn snapshot_request_shape_manual_compact_without_previous_user_messages() {
    skip_if_no_network!();

    let server = start_mock_server().await;

    let compact_turn = sse(vec![
        ev_assistant_message("m1", "MANUAL_EMPTY_SUMMARY"),
        ev_completed_with_tokens("r1", /*total_tokens*/ 90),
    ]);
    let follow_up_turn = sse(vec![
        ev_assistant_message("m2", FINAL_REPLY),
        ev_completed_with_tokens("r2", /*total_tokens*/ 80),
    ]);
    let request_log = mount_sse_sequence(&server, vec![compact_turn, follow_up_turn]).await;

    let model_provider = responses_mock_model_provider(&server);
    let codex = test_codex()
        .with_config(move |config| {
            config.model_provider = model_provider;
            set_test_compact_prompt(config);
        })
        .build(&server)
        .await
        .expect("build codex")
        .codex;

    codex.submit(Op::Compact).await.expect("run /compact");
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "AFTER_MANUAL_EMPTY_COMPACT".to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            model_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await
        .expect("submit follow-up user input");
    wait_for_event(&codex, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;

    let requests = request_log.requests();
    assert_eq!(
        requests.len(),
        2,
        "expected manual /compact request and follow-up turn request"
    );

    insta::assert_snapshot!(
        "manual_compact_without_prev_user_shapes",
        format_labeled_requests_snapshot(
            "Manual /compact with no prior user turn currently still issues a compaction request; follow-up turn carries canonical context and the new user message.",
            &[
                ("Local Compaction Request", &requests[0]),
                ("Local Post-Compaction History Layout", &requests[1]),
            ]
        )
    );
}
