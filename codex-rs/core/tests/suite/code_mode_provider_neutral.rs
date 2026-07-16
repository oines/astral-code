#![allow(clippy::expect_used)]

use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use anyhow::Result;
use codex_features::Feature;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;
use codex_protocol::ThreadId;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::TranscriptItem;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::RolloutLine;
use codex_protocol::protocol::SessionMeta;
use codex_protocol::protocol::SessionMetaLine;
use core_test_support::responses;
use core_test_support::responses::ResponseMock;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use wiremock::Mock;
use wiremock::Respond;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

const CODE: &str = "text('provider-neutral-exec');";
const CALL_ID: &str = "call-provider-neutral-exec";
const LEGACY_OUTPUT: &str = "legacy-provider-neutral-output";

#[derive(Clone, Copy)]
enum CodeModeState {
    Enabled,
    Disabled,
}

fn provider(server: &wiremock::MockServer, wire_api: WireApi) -> ModelProviderInfo {
    ModelProviderInfo {
        name: format!("{wire_api:?} test provider"),
        base_url: Some(format!("{}/v1", server.uri())),
        wire_api,
        supports_websockets: false,
        ..ModelProviderInfo::default()
    }
}

fn chat_completions_exec_sse() -> String {
    responses::chat_completions_sse(vec![json!({
        "id": "chatcmpl-exec",
        "model": "astral-test-model",
        "choices": [{
            "delta": {
                "role": "assistant",
                "tool_calls": [{
                    "index": 0,
                    "id": CALL_ID,
                    "type": "function",
                    "function": {
                        "name": "exec",
                        "arguments": serde_json::to_string(&json!({ "input": CODE }))
                            .expect("serialize exec arguments"),
                    },
                }],
            },
            "finish_reason": "tool_calls",
        }],
    })])
}

fn anthropic_sse(events: impl IntoIterator<Item = Value>) -> String {
    events
        .into_iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect()
}

fn anthropic_exec_sse() -> String {
    anthropic_sse([
        json!({
            "type": "message_start",
            "message": {
                "id": "msg-exec",
                "model": "astral-test-model",
                "usage": { "input_tokens": 1 }
            }
        }),
        json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {
                "type": "tool_use",
                "id": CALL_ID,
                "name": "exec",
                "input": {}
            }
        }),
        json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {
                "type": "input_json_delta",
                "partial_json": serde_json::to_string(&json!({ "input": CODE }))
                    .expect("serialize exec arguments")
            }
        }),
        json!({ "type": "content_block_stop", "index": 0 }),
        json!({
            "type": "message_delta",
            "delta": { "stop_reason": "tool_use" },
            "usage": { "output_tokens": 1 }
        }),
        json!({ "type": "message_stop" }),
    ])
}

fn anthropic_text_sse(text: &str) -> String {
    anthropic_sse([
        json!({
            "type": "message_start",
            "message": {
                "id": "msg-done",
                "model": "astral-test-model",
                "usage": { "input_tokens": 1 }
            }
        }),
        json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": { "type": "text", "text": "" }
        }),
        json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "text_delta", "text": text }
        }),
        json!({ "type": "content_block_stop", "index": 0 }),
        json!({
            "type": "message_delta",
            "delta": { "stop_reason": "end_turn" },
            "usage": { "output_tokens": 1 }
        }),
        json!({ "type": "message_stop" }),
    ])
}

async fn mount_anthropic_sequence(
    server: &wiremock::MockServer,
    bodies: Vec<String>,
) -> ResponseMock {
    struct SequenceResponder {
        next: AtomicUsize,
        bodies: Vec<String>,
    }

    impl Respond for SequenceResponder {
        fn respond(&self, _: &wiremock::Request) -> ResponseTemplate {
            let index = self.next.fetch_add(1, Ordering::SeqCst);
            let body = self
                .bodies
                .get(index)
                .unwrap_or_else(|| panic!("no Anthropic response for request {index}"));
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(body.clone(), "text/event-stream")
        }
    }

    let request_count = bodies.len() as u64;
    let response_mock = ResponseMock::new();
    Mock::given(method("POST"))
        .and(path_regex(".*/messages$"))
        .and(response_mock.clone())
        .respond_with(SequenceResponder {
            next: AtomicUsize::new(0),
            bodies,
        })
        .up_to_n_times(request_count)
        .expect(request_count)
        .mount(server)
        .await;
    response_mock
}

fn assert_exec_function_schema(body: &Value) {
    let exec = body
        .get("tools")
        .and_then(Value::as_array)
        .and_then(|tools| {
            tools.iter().find(|tool| {
                tool.pointer("/function/name")
                    .or_else(|| tool.get("name"))
                    .and_then(Value::as_str)
                    == Some("exec")
            })
        })
        .expect("exec should be projected as a function tool");
    let schema = exec
        .pointer("/function/parameters")
        .or_else(|| exec.get("input_schema"))
        .expect("exec function schema");
    assert_eq!(
        schema.pointer("/properties/input/type"),
        Some(&json!("string"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chat_completions_exec_function_payload_runs_and_returns_tool_output() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    let request_mock = responses::mount_chat_completions_sse_sequence(
        &server,
        vec![
            chat_completions_exec_sse(),
            responses::chat_completions_text_sse("done"),
        ],
    )
    .await;
    let model_provider = provider(&server, WireApi::ChatCompletions);
    let test = test_codex()
        .with_model("test-gpt-5.1-codex")
        .with_config(move |config| {
            config.model_provider = model_provider;
            config
                .features
                .enable(Feature::CodeMode)
                .expect("enable code mode");
        })
        .build(&server)
        .await?;

    test.submit_turn("run provider-neutral exec").await?;

    let requests = request_mock.requests();
    assert_eq!(requests.len(), 2);
    assert_exec_function_schema(&requests[0].body_json());
    let follow_up = requests[1].body_json();
    let tool_output = follow_up
        .get("messages")
        .and_then(Value::as_array)
        .and_then(|messages| {
            messages.iter().find(|message| {
                message.get("role").and_then(Value::as_str) == Some("tool")
                    && message.get("tool_call_id").and_then(Value::as_str) == Some(CALL_ID)
            })
        })
        .expect("function output should be projected as a Chat Completions tool message");
    assert!(tool_output.to_string().contains("provider-neutral-exec"));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn anthropic_messages_exec_function_payload_runs_and_returns_tool_result() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    let request_mock = mount_anthropic_sequence(
        &server,
        vec![anthropic_exec_sse(), anthropic_text_sse("done")],
    )
    .await;
    let model_provider = provider(&server, WireApi::AnthropicMessages);
    let test = test_codex()
        .with_model("test-gpt-5.1-codex")
        .with_config(move |config| {
            config.model_provider = model_provider;
            config
                .features
                .enable(Feature::CodeMode)
                .expect("enable code mode");
        })
        .build(&server)
        .await?;

    test.submit_turn("run provider-neutral exec").await?;

    let requests = request_mock.requests();
    assert_eq!(requests.len(), 2);
    assert_exec_function_schema(&requests[0].body_json());
    let follow_up = requests[1].body_json();
    let tool_result = follow_up
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|message| message.get("content").and_then(Value::as_array))
        .flatten()
        .find(|content| {
            content.get("type").and_then(Value::as_str) == Some("tool_result")
                && content.get("tool_use_id").and_then(Value::as_str) == Some(CALL_ID)
        })
        .expect("function output should be projected as an Anthropic tool_result");
    assert!(tool_result.to_string().contains("provider-neutral-exec"));

    Ok(())
}

async fn legacy_custom_exec_history_replayed_to_function_provider(
    target_wire_api: WireApi,
    code_mode: CodeModeState,
) -> Result<Value> {
    let target_server = responses::start_mock_server().await;
    let target_mock = match target_wire_api {
        WireApi::ChatCompletions => {
            responses::mount_chat_completions_sse_sequence(
                &target_server,
                vec![responses::chat_completions_text_sse("second turn done")],
            )
            .await
        }
        WireApi::AnthropicMessages => {
            mount_anthropic_sequence(&target_server, vec![anthropic_text_sse("second turn done")])
                .await
        }
        WireApi::Responses => panic!("target must use a function-only wire API"),
    };

    let target_provider = provider(&target_server, target_wire_api);
    let codex_home = Arc::new(TempDir::new()?);
    let rollout_path = codex_home.path().join("legacy-custom-exec-rollout.jsonl");
    let rollout = [
        RolloutLine {
            timestamp: "2024-01-01T00:00:00.000Z".to_string(),
            item: RolloutItem::SessionMeta(SessionMetaLine {
                meta: SessionMeta {
                    id: ThreadId::default(),
                    timestamp: "2024-01-01T00:00:00Z".to_string(),
                    cwd: ".".into(),
                    originator: "test_originator".to_string(),
                    cli_version: "test_version".to_string(),
                    ..Default::default()
                },
                git: None,
            }),
        },
        RolloutLine {
            timestamp: "2024-01-01T00:00:01.000Z".to_string(),
            item: RolloutItem::TranscriptItem(TranscriptItem::CustomToolCall {
                id: Some("custom-exec".to_string()),
                status: Some("completed".to_string()),
                call_id: CALL_ID.to_string(),
                name: "exec".to_string(),
                input: CODE.to_string(),
            }),
        },
        RolloutLine {
            timestamp: "2024-01-01T00:00:02.000Z".to_string(),
            item: RolloutItem::TranscriptItem(TranscriptItem::CustomToolCallOutput {
                call_id: CALL_ID.to_string(),
                name: Some("exec".to_string()),
                output: FunctionCallOutputPayload::from_text(LEGACY_OUTPUT.to_string()),
            }),
        },
    ];
    let mut file = std::fs::File::create(&rollout_path)?;
    for line in rollout {
        writeln!(file, "{}", serde_json::to_string(&line)?)?;
    }

    let mut builder = test_codex()
        .with_model("test-gpt-5.1-codex")
        .with_config(move |config| {
            config.model_provider = target_provider;
            if matches!(code_mode, CodeModeState::Enabled) {
                config
                    .features
                    .enable(Feature::CodeMode)
                    .expect("enable code mode");
            }
        });
    let test = builder
        .resume(&target_server, codex_home, rollout_path)
        .await?;
    test.submit_turn("continue after legacy custom exec")
        .await?;

    Ok(target_mock.single_request().body_json())
}

fn assert_exec_tool_not_advertised(body: &Value) {
    let exec_advertised = body
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|tool| {
            tool.pointer("/function/name").and_then(Value::as_str) == Some("exec")
                || tool.get("name").and_then(Value::as_str) == Some("exec")
        });
    assert!(
        !exec_advertised,
        "legacy replay must not require the current tool plan"
    );
}

fn assert_exec_tool_advertisement(body: &Value, code_mode: CodeModeState) {
    match code_mode {
        CodeModeState::Enabled => assert_exec_function_schema(body),
        CodeModeState::Disabled => assert_exec_tool_not_advertised(body),
    }
}

fn assert_chat_completions_legacy_exec_input(body: &Value) -> Result<()> {
    let arguments = body
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|message| message.get("tool_calls").and_then(Value::as_array))
        .flatten()
        .find(|call| call.pointer("/function/name").and_then(Value::as_str) == Some("exec"))
        .and_then(|call| call.pointer("/function/arguments"))
        .and_then(Value::as_str)
        .expect("legacy custom exec should become a Chat Completions function call");
    assert_eq!(
        serde_json::from_str::<Value>(arguments)?,
        json!({ "input": CODE })
    );
    assert!(body.to_string().contains(LEGACY_OUTPUT));
    Ok(())
}

fn assert_anthropic_legacy_exec_input(body: &Value) {
    let input = body
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|message| message.get("content").and_then(Value::as_array))
        .flatten()
        .find(|block| {
            block.get("type").and_then(Value::as_str) == Some("tool_use")
                && block.get("name").and_then(Value::as_str) == Some("exec")
        })
        .and_then(|block| block.get("input"))
        .expect("legacy custom exec should become an Anthropic function call");
    assert_eq!(input, &json!({ "input": CODE }));
    assert!(body.to_string().contains(LEGACY_OUTPUT));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_custom_exec_history_resumes_to_chat_completions_with_code_mode_disabled()
-> Result<()> {
    skip_if_no_network!(Ok(()));

    let code_mode = CodeModeState::Disabled;
    let body = legacy_custom_exec_history_replayed_to_function_provider(
        WireApi::ChatCompletions,
        code_mode,
    )
    .await?;
    assert_exec_tool_advertisement(&body, code_mode);
    assert_chat_completions_legacy_exec_input(&body)?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_custom_exec_history_resumes_to_chat_completions_with_code_mode_enabled()
-> Result<()> {
    skip_if_no_network!(Ok(()));

    let code_mode = CodeModeState::Enabled;
    let body = legacy_custom_exec_history_replayed_to_function_provider(
        WireApi::ChatCompletions,
        code_mode,
    )
    .await?;
    assert_exec_tool_advertisement(&body, code_mode);
    assert_chat_completions_legacy_exec_input(&body)?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_custom_exec_history_resumes_to_anthropic_with_code_mode_disabled() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let code_mode = CodeModeState::Disabled;
    let body = legacy_custom_exec_history_replayed_to_function_provider(
        WireApi::AnthropicMessages,
        code_mode,
    )
    .await?;
    assert_exec_tool_advertisement(&body, code_mode);
    assert_anthropic_legacy_exec_input(&body);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_custom_exec_history_resumes_to_anthropic_with_code_mode_enabled() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let code_mode = CodeModeState::Enabled;
    let body = legacy_custom_exec_history_replayed_to_function_provider(
        WireApi::AnthropicMessages,
        code_mode,
    )
    .await?;
    assert_exec_tool_advertisement(&body, code_mode);
    assert_anthropic_legacy_exec_input(&body);

    Ok(())
}
