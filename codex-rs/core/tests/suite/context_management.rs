use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use anyhow::Result;
use codex_features::Feature;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;
use core_test_support::responses;
use core_test_support::responses::ResponseMock;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::request_tool_names;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use serde_json::Value;
use serde_json::json;
use wiremock::Mock;
use wiremock::Respond;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

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

fn provider(server: &wiremock::MockServer, wire_api: WireApi) -> ModelProviderInfo {
    ModelProviderInfo {
        name: format!("{wire_api:?} context management test provider"),
        base_url: Some(format!("{}/v1", server.uri())),
        wire_api,
        supports_websockets: false,
        ..ModelProviderInfo::default()
    }
}

fn chat_completions_new_context_sse() -> String {
    responses::chat_completions_sse(vec![json!({
        "id": "chatcmpl-new-context",
        "model": "astral-test-model",
        "choices": [{
            "delta": {
                "role": "assistant",
                "tool_calls": [{
                    "index": 0,
                    "id": "chat-new-context-call",
                    "type": "function",
                    "function": { "name": "new_context", "arguments": "{}" },
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

fn anthropic_new_context_sse() -> String {
    anthropic_sse([
        json!({
            "type": "message_start",
            "message": {
                "id": "msg-new-context",
                "model": "astral-test-model",
                "usage": { "input_tokens": 1 }
            }
        }),
        json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {
                "type": "tool_use",
                "id": "anthropic-new-context-call",
                "name": "new_context",
                "input": {}
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
                "id": "msg-after-context-reset",
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_context_restarts_sampling_in_same_turn_without_old_transcript() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-old-window"),
                ev_function_call("new-context-call", "new_context", "{}"),
                ev_completed("resp-old-window"),
            ]),
            sse(vec![
                ev_response_created("resp-new-window"),
                ev_assistant_message("message-new-window", "continued after rollover"),
                ev_completed("resp-new-window"),
            ]),
        ],
    )
    .await;
    let test = test_codex()
        .with_config(|config| {
            config.model_context_window = Some(10_000);
            config
                .features
                .enable(Feature::ContextManagement)
                .expect("context management feature should be configurable");
        })
        .build(&server)
        .await?;

    let old_window_marker = "OLD_WINDOW_TRANSCRIPT_MARKER";
    test.submit_turn(old_window_marker).await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    assert!(request_tool_names(&requests[0].body_json()).contains(&"new_context".to_string()));
    assert!(requests[0].body_contains_text(old_window_marker));
    assert!(requests[0].body_contains_text("[id: "));
    assert!(!requests[1].body_contains_text(old_window_marker));
    assert!(requests[1].body_contains_text("<context_window>"));
    assert!(requests[1].body_contains_text("[id: "));
    assert!(requests[0].body_contains_text("Take incremental notes while you work"));
    assert!(requests[1].body_contains_text("Take incremental notes while you work"));
    assert!(requests[1].body_contains_text("prefer `history.read_item`"));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_turn_can_cross_three_clean_context_windows() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-window-one"),
                ev_function_call("new-context-one", "new_context", "{}"),
                ev_completed("resp-window-one"),
            ]),
            sse(vec![
                ev_response_created("resp-window-two"),
                ev_function_call("new-context-two", "new_context", "{}"),
                ev_completed("resp-window-two"),
            ]),
            sse(vec![
                ev_response_created("resp-window-three"),
                ev_assistant_message("message-window-three", "finished across three windows"),
                ev_completed("resp-window-three"),
            ]),
        ],
    )
    .await;
    let test = test_codex()
        .with_config(|config| {
            config.model_context_window = Some(10_000);
            config
                .features
                .enable(Feature::ContextManagement)
                .expect("context management feature should be configurable");
        })
        .build(&server)
        .await?;

    let first_window_marker = "THREE_WINDOW_ORIGINAL_TRANSCRIPT";
    test.submit_turn(first_window_marker).await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].body_contains_text(first_window_marker));
    assert!(!requests[1].body_contains_text(first_window_marker));
    assert!(!requests[2].body_contains_text(first_window_marker));
    assert!(requests[1].body_contains_text("<context_window>"));
    assert!(requests[2].body_contains_text("<context_window>"));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clean_window_can_recover_old_text_from_authoritative_history_without_notes() -> Result<()>
{
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let marker = "HISTORY_RECOVERY_MARKER_FROM_OLD_WINDOW";
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-before-history-reset"),
                ev_function_call("new-context-history", "new_context", "{}"),
                ev_completed("resp-before-history-reset"),
            ]),
            sse(vec![
                ev_response_created("resp-history-search"),
                ev_function_call(
                    "history-search-call",
                    "history__search_contents",
                    &serde_json::to_string(&json!({"query": marker}))?,
                ),
                ev_completed("resp-history-search"),
            ]),
            sse(vec![
                ev_response_created("resp-after-history-search"),
                ev_assistant_message("message-after-history-search", "recovered from history"),
                ev_completed("resp-after-history-search"),
            ]),
        ],
    )
    .await;
    let test = test_codex()
        .with_config(|config| {
            config.model_context_window = Some(10_000);
            config
                .features
                .enable(Feature::ContextManagement)
                .expect("context management feature should be configurable");
        })
        .build(&server)
        .await?;

    test.submit_turn(marker).await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].body_contains_text(marker));
    assert!(!requests[1].body_contains_text(marker));
    assert!(
        request_tool_names(&requests[1].body_json())
            .contains(&"history__search_contents".to_string())
    );
    assert!(requests[2].body_contains_text(marker));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clean_window_injects_only_note_path_until_model_reads_checkpoint() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let note_path = "task/checkpoint.md";
    let note_marker = "PRIVATE_NOTE_CONTENT_RECOVERED_AFTER_RESET";
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-note-write"),
                ev_function_call(
                    "note-write-call",
                    "notes__write_file",
                    &serde_json::to_string(&json!({
                        "path": note_path,
                        "text": note_marker,
                    }))?,
                ),
                ev_completed("resp-note-write"),
            ]),
            sse(vec![
                ev_response_created("resp-note-reset"),
                ev_function_call("new-context-after-note", "new_context", "{}"),
                ev_completed("resp-note-reset"),
            ]),
            sse(vec![
                ev_response_created("resp-note-read"),
                ev_function_call(
                    "note-read-call",
                    "notes__read_file",
                    &serde_json::to_string(&json!({"path": note_path}))?,
                ),
                ev_completed("resp-note-read"),
            ]),
            sse(vec![
                ev_response_created("resp-note-finished"),
                ev_assistant_message("message-note-finished", "continued from note"),
                ev_completed("resp-note-finished"),
            ]),
        ],
    )
    .await;
    let test = test_codex()
        .with_config(|config| {
            config.model_context_window = Some(10_000);
            config
                .features
                .enable(Feature::ContextManagement)
                .expect("context management feature should be configurable");
        })
        .build(&server)
        .await?;

    test.submit_turn("write a checkpoint, reset, and recover it")
        .await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 4);
    assert!(requests[1].body_contains_text(note_marker));
    assert!(!requests[2].body_contains_text(note_marker));
    assert!(requests[2].body_contains_text(note_path));
    assert!(requests[3].body_contains_text(note_marker));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chat_completions_clean_reset_keeps_same_turn_and_drops_old_window() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let request_mock = responses::mount_chat_completions_sse_sequence(
        &server,
        vec![
            chat_completions_new_context_sse(),
            responses::chat_completions_text_sse("continued after chat reset"),
        ],
    )
    .await;
    let model_provider = provider(&server, WireApi::ChatCompletions);
    let test = test_codex()
        .with_model("test-gpt-5.1-codex")
        .with_config(move |config| {
            config.model_provider = model_provider;
            config.model_context_window = Some(10_000);
            config
                .features
                .enable(Feature::ContextManagement)
                .expect("context management feature should be configurable");
        })
        .build(&server)
        .await?;

    let marker = "CHAT_COMPLETIONS_OLD_WINDOW_MARKER";
    test.submit_turn(marker).await?;

    let requests = request_mock.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].body_contains_text(marker));
    assert!(!requests[1].body_contains_text(marker));
    assert!(requests[1].body_contains_text("<context_window>"));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn anthropic_messages_clean_reset_keeps_same_turn_and_drops_old_window() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let request_mock = mount_anthropic_sequence(
        &server,
        vec![
            anthropic_new_context_sse(),
            anthropic_text_sse("continued after anthropic reset"),
        ],
    )
    .await;
    let model_provider = provider(&server, WireApi::AnthropicMessages);
    let test = test_codex()
        .with_model("test-gpt-5.1-codex")
        .with_config(move |config| {
            config.model_provider = model_provider;
            config.model_context_window = Some(10_000);
            config
                .features
                .enable(Feature::ContextManagement)
                .expect("context management feature should be configurable");
        })
        .build(&server)
        .await?;

    let marker = "ANTHROPIC_OLD_WINDOW_MARKER";
    test.submit_turn(marker).await?;

    let requests = request_mock.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].body_contains_text(marker));
    assert!(!requests[1].body_contains_text(marker));
    assert!(requests[1].body_contains_text("<context_window>"));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exhausted_checkpoint_reserve_forces_new_context_before_follow_up_sampling() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-old-window"),
                ev_function_call("remaining-call", "get_context_remaining", "{}"),
                ev_completed_with_usage("resp-old-window", 10_000, 10),
            ]),
            sse(vec![
                ev_response_created("resp-new-window"),
                ev_assistant_message("message-new-window", "continued after forced rollover"),
                ev_completed("resp-new-window"),
            ]),
        ],
    )
    .await;
    let test = test_codex()
        .with_config(|config| {
            config.model_context_window = Some(10_000);
            config.model_auto_compact_token_limit = Some(100);
            config
                .features
                .enable(Feature::ContextManagement)
                .expect("context management feature should be configurable");
        })
        .build(&server)
        .await?;

    let old_window_marker = "FORCED_OLD_WINDOW_MARKER";
    test.submit_turn(old_window_marker).await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].body_contains_text(old_window_marker));
    assert!(!requests[1].body_contains_text(old_window_marker));
    assert!(requests[1].body_contains_text("<context_window>"));

    Ok(())
}
