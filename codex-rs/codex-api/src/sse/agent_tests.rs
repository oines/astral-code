use codex_agent_protocol::AgentStreamEvent;
use codex_agent_protocol::ContentBlock;
use codex_agent_protocol::ContentDelta;
use codex_agent_protocol::StopReason;
use codex_agent_protocol::TokenUsage as AgentTokenUsage;
use codex_protocol::models::ContentItem;
use codex_protocol::models::TranscriptItem;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::collections::BTreeMap;
use tokio::sync::mpsc;
use tokio::time::Duration;

use super::AgentStreamFormat;
use super::AgentStreamMapper;
use super::ModelStreamEvent;
use super::process_sse;

#[test]
fn mapper_streams_text_with_lazy_content_block_start() {
    let mut mapper = AgentStreamMapper::default();

    let events = mapper
        .process_event(AgentStreamEvent::MessageStart {
            id: Some("msg_1".to_string()),
            model: Some("astral-fast".to_string()),
            usage: None,
        })
        .expect("message start maps");
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], super::ModelStreamEvent::Created));
    assert!(matches!(
        &events[1],
        super::ModelStreamEvent::ServerModel(model) if model == "astral-fast"
    ));

    let events = mapper
        .process_event(AgentStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text {
                text: "hello".to_string(),
            },
        })
        .expect("text delta maps");
    assert_eq!(events.len(), 2);
    let super::ModelStreamEvent::OutputItemAdded(TranscriptItem::Message { content, .. }) =
        &events[0]
    else {
        panic!("expected assistant message item start, got {:?}", events[0]);
    };
    assert_eq!(
        content,
        &vec![ContentItem::OutputText {
            text: String::new()
        }]
    );
    assert!(matches!(
        &events[1],
        super::ModelStreamEvent::OutputTextDelta(delta) if delta == "hello"
    ));

    let events = mapper
        .process_event(AgentStreamEvent::MessageStop {
            stop_reason: Some(StopReason::EndTurn),
            usage: Some(AgentTokenUsage {
                input_tokens: Some(12),
                output_tokens: Some(7),
                cache_creation_input_tokens: None,
                cache_read_input_tokens: Some(3),
            }),
        })
        .expect("message stop maps");
    assert_eq!(events.len(), 2);
    let super::ModelStreamEvent::OutputItemDone(TranscriptItem::Message { content, .. }) =
        &events[0]
    else {
        panic!("expected assistant message item done, got {:?}", events[0]);
    };
    assert_eq!(
        content,
        &vec![ContentItem::OutputText {
            text: "hello".to_string(),
        }]
    );
    let super::ModelStreamEvent::Completed {
        response_id,
        token_usage,
        end_turn,
    } = &events[1]
    else {
        panic!("expected completed event, got {:?}", events[1]);
    };
    assert_eq!(response_id, "msg_1");
    assert_eq!(*end_turn, Some(true));
    let usage = token_usage.as_ref().expect("token usage present");
    assert_eq!(usage.input_tokens, 15);
    assert_eq!(usage.cached_input_tokens, 3);
    assert_eq!(usage.output_tokens, 7);
    assert_eq!(usage.total_tokens, 22);
}

#[test]
fn mapper_merges_message_start_and_stop_usage() {
    let mut mapper = AgentStreamMapper::default();

    mapper
        .process_event(AgentStreamEvent::MessageStart {
            id: Some("msg_1".to_string()),
            model: None,
            usage: Some(AgentTokenUsage {
                input_tokens: Some(19),
                output_tokens: None,
                cache_creation_input_tokens: Some(5),
                cache_read_input_tokens: None,
            }),
        })
        .expect("message start maps");

    let events = mapper
        .process_event(AgentStreamEvent::MessageStop {
            stop_reason: Some(StopReason::EndTurn),
            usage: Some(AgentTokenUsage {
                input_tokens: Some(10),
                output_tokens: Some(7),
                cache_creation_input_tokens: None,
                cache_read_input_tokens: Some(3),
            }),
        })
        .expect("message stop maps");

    let super::ModelStreamEvent::Completed { token_usage, .. } = &events[0] else {
        panic!("expected completed event, got {:?}", events[0]);
    };
    let usage = token_usage.as_ref().expect("token usage present");
    assert_eq!(usage.input_tokens, 18);
    assert_eq!(usage.cached_input_tokens, 3);
    assert_eq!(usage.output_tokens, 7);
    assert_eq!(usage.total_tokens, 25);
}

#[test]
fn mapper_preserves_anthropic_reasoning_signature() {
    let mut mapper = AgentStreamMapper::default();

    let events = mapper
        .process_event(AgentStreamEvent::ContentBlockStart {
            index: 0,
            block: ContentBlock::Reasoning {
                text: "thinking".to_string(),
                signature: None,
            },
        })
        .expect("reasoning start maps");
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        super::ModelStreamEvent::OutputItemAdded(TranscriptItem::Reasoning { .. })
    ));

    let events = mapper
        .process_event(AgentStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ReasoningSignature {
                signature: "sig_opaque".to_string(),
            },
        })
        .expect("reasoning signature maps");
    assert!(events.is_empty());

    let events = mapper
        .process_event(AgentStreamEvent::ContentBlockStop { index: 0 })
        .expect("reasoning stop maps");
    assert_eq!(events.len(), 1);
    let super::ModelStreamEvent::OutputItemDone(TranscriptItem::Reasoning {
        encrypted_content,
        provider_metadata,
        ..
    }) = &events[0]
    else {
        panic!("expected reasoning item done, got {:?}", events[0]);
    };
    assert_eq!(encrypted_content, &None);
    assert_eq!(
        provider_metadata
            .as_ref()
            .and_then(|metadata| metadata.anthropic_signature.as_deref()),
        Some("sig_opaque")
    );
}

#[test]
fn mapper_streams_tool_arguments_and_finishes_function_call() {
    let mut mapper = AgentStreamMapper::default();

    let events = mapper
        .process_event(AgentStreamEvent::ContentBlockStart {
            index: 1,
            block: ContentBlock::ToolUse {
                id: "toolu_1".to_string(),
                name: "Bash".to_string(),
                input: json!({}),
            },
        })
        .expect("tool start maps");
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        super::ModelStreamEvent::OutputItemAdded(TranscriptItem::FunctionCall {
            call_id,
            name,
            ..
        }) if call_id == "toolu_1" && name == "Bash"
    ));

    let events = mapper
        .process_event(AgentStreamEvent::ContentBlockDelta {
            index: 1,
            delta: ContentDelta::ToolInputJson {
                partial_json: r#"{"command":"pwd"}"#.to_string(),
            },
        })
        .expect("tool input maps");
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        super::ModelStreamEvent::ToolCallInputDelta {
            item_id,
            call_id: Some(call_id),
            delta,
        } if item_id == "toolu_1" && call_id == "toolu_1" && delta == r#"{"command":"pwd"}"#
    ));

    let events = mapper
        .process_event(AgentStreamEvent::ContentBlockStop { index: 1 })
        .expect("tool stop maps");
    assert_eq!(events.len(), 1);
    let super::ModelStreamEvent::OutputItemDone(TranscriptItem::FunctionCall {
        call_id,
        name,
        arguments,
        namespace,
        ..
    }) = &events[0]
    else {
        panic!("expected function call done, got {:?}", events[0]);
    };
    assert_eq!(call_id, "toolu_1");
    assert_eq!(name, "Bash");
    assert_eq!(namespace, &None);
    assert_eq!(arguments, r#"{"command":"pwd"}"#);
}

#[test]
fn mapper_restores_anthropic_tool_name_aliases() {
    let mut mapper = AgentStreamMapper::new(BTreeMap::from([(
        "mcp__server_tool__1234567890abcdef".to_string(),
        "mcp__server__tool".to_string(),
    )]));

    let events = mapper
        .process_event(AgentStreamEvent::ContentBlockStart {
            index: 1,
            block: ContentBlock::ToolUse {
                id: "toolu_1".to_string(),
                name: "mcp__server_tool__1234567890abcdef".to_string(),
                input: json!({}),
            },
        })
        .expect("tool start maps");

    assert!(matches!(
        &events[0],
        super::ModelStreamEvent::OutputItemAdded(TranscriptItem::FunctionCall {
            name,
            ..
        }) if name == "mcp__server__tool"
    ));

    let events = mapper
        .process_event(AgentStreamEvent::ContentBlockStop { index: 1 })
        .expect("tool stop maps");
    assert!(matches!(
        &events[0],
        super::ModelStreamEvent::OutputItemDone(TranscriptItem::FunctionCall {
            name,
            ..
        }) if name == "mcp__server__tool"
    ));
}

#[test]
fn mapper_marks_tool_use_stop_as_follow_up_required() {
    let mut mapper = AgentStreamMapper::default();
    let events = mapper
        .process_event(AgentStreamEvent::MessageStop {
            stop_reason: Some(StopReason::ToolUse),
            usage: None,
        })
        .expect("tool use stop maps");

    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        super::ModelStreamEvent::Completed {
            end_turn: Some(false),
            ..
        }
    ));
}

#[test]
fn mapper_warns_when_model_stops_at_max_tokens() {
    let mut mapper = AgentStreamMapper::default();
    let events = mapper
        .process_event(AgentStreamEvent::MessageStop {
            stop_reason: Some(StopReason::MaxTokens),
            usage: None,
        })
        .expect("max_tokens stop maps");

    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[0],
        super::ModelStreamEvent::Warning(message) if message.contains("max_tokens")
    ));
    assert!(matches!(
        &events[1],
        super::ModelStreamEvent::Completed {
            end_turn: Some(true),
            ..
        }
    ));
}

#[test]
fn mapper_turns_provider_error_stop_into_stream_error() {
    let mut mapper = AgentStreamMapper::default();
    let error = mapper
        .process_event(AgentStreamEvent::MessageStop {
            stop_reason: Some(StopReason::Error {
                message: "rate limited".to_string(),
            }),
            usage: None,
        })
        .expect_err("error stop should fail stream");

    assert_eq!(error.to_string(), "stream error: rate limited");
}

#[tokio::test]
async fn chat_stream_merges_finish_reason_with_empty_choices_usage_chunk() {
    let chunks = vec![
        Ok(bytes::Bytes::from(format!(
            "data: {}\n\n",
            json!({
                "id": "chatcmpl_1",
                "model": "astral-fast",
                "choices": [{
                    "delta": { "role": "assistant", "content": "hello" }
                }]
            })
        ))),
        Ok(bytes::Bytes::from(format!(
            "data: {}\n\n",
            json!({
                "choices": [{
                    "delta": {},
                    "finish_reason": "stop"
                }]
            })
        ))),
        Ok(bytes::Bytes::from(format!(
            "data: {}\n\n",
            json!({
                "choices": [],
                "usage": {
                    "prompt_tokens": 13,
                    "completion_tokens": 5,
                    "prompt_tokens_details": { "cached_tokens": 8 }
                }
            })
        ))),
        Ok(bytes::Bytes::from("data: [DONE]\n\n")),
    ];
    let stream = futures::stream::iter(chunks).boxed() as codex_client::ByteStream;
    let (tx_event, mut rx_event) = mpsc::channel(16);

    process_sse(
        stream,
        tx_event,
        Duration::from_secs(5),
        None,
        AgentStreamFormat::ChatCompletions,
        Default::default(),
    )
    .await;

    let mut events = Vec::new();
    while let Some(event) = rx_event.recv().await {
        events.push(event.expect("event should map"));
    }

    let completed = events
        .iter()
        .find_map(|event| match event {
            ModelStreamEvent::Completed {
                token_usage,
                end_turn,
                ..
            } => Some((token_usage.as_ref().expect("usage present"), end_turn)),
            _ => None,
        })
        .expect("completed event");

    assert_eq!(completed.0.input_tokens, 13);
    assert_eq!(completed.0.cached_input_tokens, 8);
    assert_eq!(completed.0.output_tokens, 5);
    assert_eq!(*completed.1, Some(true));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ModelStreamEvent::Completed { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn chat_stream_keeps_streaming_when_content_chunks_include_usage() {
    let chunks = vec![
        Ok(bytes::Bytes::from(format!(
            "data: {}\n\n",
            json!({
                "id": "chatcmpl_1",
                "model": "astral-fast",
                "choices": [{
                    "delta": { "role": "assistant", "content": "hel" },
                    "finish_reason": null
                }],
                "usage": {
                    "prompt_tokens": 13,
                    "completion_tokens": 1,
                    "prompt_tokens_details": { "cached_tokens": 8 }
                }
            })
        ))),
        Ok(bytes::Bytes::from(format!(
            "data: {}\n\n",
            json!({
                "choices": [{
                    "delta": { "content": "lo" },
                    "finish_reason": null
                }],
                "usage": {
                    "prompt_tokens": 13,
                    "completion_tokens": 2,
                    "prompt_tokens_details": { "cached_tokens": 8 }
                }
            })
        ))),
        Ok(bytes::Bytes::from(format!(
            "data: {}\n\n",
            json!({
                "choices": [{
                    "delta": {},
                    "finish_reason": "stop"
                }]
            })
        ))),
        Ok(bytes::Bytes::from("data: [DONE]\n\n")),
    ];
    let stream = futures::stream::iter(chunks).boxed() as codex_client::ByteStream;
    let (tx_event, mut rx_event) = mpsc::channel(16);

    process_sse(
        stream,
        tx_event,
        Duration::from_secs(5),
        None,
        AgentStreamFormat::ChatCompletions,
        Default::default(),
    )
    .await;

    let mut events = Vec::new();
    while let Some(event) = rx_event.recv().await {
        events.push(event.expect("event should map"));
    }

    let done_message = events
        .iter()
        .find_map(|event| match event {
            ModelStreamEvent::OutputItemDone(TranscriptItem::Message { content, .. }) => {
                Some(content)
            }
            _ => None,
        })
        .expect("message done event");
    assert_eq!(
        done_message,
        &vec![ContentItem::OutputText {
            text: "hello".to_string()
        }]
    );

    let completed = events
        .iter()
        .find_map(|event| match event {
            ModelStreamEvent::Completed {
                token_usage,
                end_turn,
                ..
            } => Some((token_usage.as_ref().expect("usage present"), end_turn)),
            _ => None,
        })
        .expect("completed event");
    assert_eq!(completed.0.input_tokens, 13);
    assert_eq!(completed.0.cached_input_tokens, 8);
    assert_eq!(completed.0.output_tokens, 2);
    assert_eq!(*completed.1, Some(true));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ModelStreamEvent::Completed { .. }))
            .count(),
        1
    );
}
