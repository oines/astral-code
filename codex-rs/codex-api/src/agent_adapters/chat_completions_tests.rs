use codex_agent_protocol::AgentMessage;
use codex_agent_protocol::AgentRequest;
use codex_agent_protocol::AgentStreamEvent;
use codex_agent_protocol::AgentTool;
use codex_agent_protocol::ContentBlock;
use codex_agent_protocol::ContentDelta;
use codex_agent_protocol::MessageRole;
use codex_agent_protocol::RequestMetadata;
use codex_agent_protocol::StopReason;
use codex_agent_protocol::TokenUsage;
use codex_agent_protocol::ToolChoice;
use codex_agent_protocol::ToolResultContent;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::collections::BTreeMap;

use super::ChatCompletionsOptions;
use super::parse_stream_chunk;
use super::to_chat_completions_request;

#[test]
fn request_maps_tool_use_and_tool_result_to_chat_shape() {
    let request = AgentRequest {
        model: "astral-large".to_string(),
        instructions: vec![ContentBlock::Text {
            text: "You are Astral-Code.".to_string(),
        }],
        messages: vec![
            AgentMessage {
                role: MessageRole::User,
                content: vec![ContentBlock::Text {
                    text: "list files".to_string(),
                }],
                id: None,
            },
            AgentMessage {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "Bash".to_string(),
                    input: json!({ "command": "ls" }),
                }],
                id: None,
            },
            AgentMessage {
                role: MessageRole::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: vec![ToolResultContent::Text {
                        text: "Cargo.toml".to_string(),
                    }],
                    is_error: false,
                }],
                id: None,
            },
        ],
        tools: vec![AgentTool {
            name: "Bash".to_string(),
            description: "Run a shell command".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": { "command": { "type": "string" } },
                "required": ["command"]
            }),
            metadata: BTreeMap::new(),
        }],
        tool_choice: ToolChoice::Auto,
        parallel_tool_calls: true,
        stream: true,
        reasoning: None,
        metadata: RequestMetadata {
            service_tier: Some("priority".to_string()),
            provider: BTreeMap::from([("temperature".to_string(), json!(0.2))]),
            ..RequestMetadata::default()
        },
    };

    assert_eq!(
        to_chat_completions_request(
            &request,
            ChatCompletionsOptions {
                max_tokens: Some(1024)
            }
        ),
        json!({
            "model": "astral-large",
            "stream": true,
            "max_tokens": 1024,
            "service_tier": "priority",
            "stream_options": { "include_usage": true },
            "temperature": 0.2,
            "messages": [
                { "role": "system", "content": "You are Astral-Code." },
                {
                    "role": "user",
                    "content": [{ "type": "text", "text": "list files" }]
                },
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "Bash",
                            "arguments": r#"{"command":"ls"}"#
                        }
                    }]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_1",
                    "content": "Cargo.toml"
                }
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "Bash",
                    "description": "Run a shell command",
                    "parameters": {
                        "type": "object",
                        "properties": { "command": { "type": "string" } },
                        "required": ["command"]
                    }
                }
            }],
            "tool_choice": "auto",
            "parallel_tool_calls": true
        })
    );
}

#[test]
fn stream_chunk_maps_text_tool_calls_finish_reason_and_usage() {
    assert_eq!(
        parse_stream_chunk(json!({
            "id": "chatcmpl_1",
            "model": "astral-fast",
            "choices": [{
                "delta": {
                    "role": "assistant",
                    "content": "hello",
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "Bash",
                            "arguments": r#"{"command":"pwd"}"#
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 12,
                "completion_tokens": 8,
                "prompt_tokens_details": { "cached_tokens": 3 }
            }
        }))
        .expect("parse stream chunk"),
        vec![
            AgentStreamEvent::MessageStart {
                id: Some("chatcmpl_1".to_string()),
                model: Some("astral-fast".to_string()),
            },
            AgentStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text {
                    text: "hello".to_string(),
                },
            },
            AgentStreamEvent::ContentBlockStart {
                index: 1,
                block: ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "Bash".to_string(),
                    input: json!({}),
                },
            },
            AgentStreamEvent::ContentBlockDelta {
                index: 1,
                delta: ContentDelta::ToolInputJson {
                    partial_json: r#"{"command":"pwd"}"#.to_string(),
                },
            },
            AgentStreamEvent::MessageStop {
                stop_reason: Some(StopReason::ToolUse),
                usage: Some(TokenUsage {
                    input_tokens: Some(12),
                    output_tokens: Some(8),
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: Some(3),
                }),
            },
        ]
    );
}

#[test]
fn stream_chunk_maps_usage_only_chunk() {
    assert_eq!(
        parse_stream_chunk(json!({
            "choices": [{ "delta": {} }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 2 }
        }))
        .expect("parse stream chunk"),
        vec![AgentStreamEvent::MessageStop {
            stop_reason: None,
            usage: Some(TokenUsage {
                input_tokens: Some(1),
                output_tokens: Some(2),
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            }),
        }]
    );
}

#[test]
fn stream_chunk_maps_openai_compatible_empty_choices_usage_chunk() {
    assert_eq!(
        parse_stream_chunk(json!({
            "choices": [],
            "usage": {
                "prompt_tokens": 13,
                "completion_tokens": 5,
                "prompt_tokens_details": { "cached_tokens": 8 }
            }
        }))
        .expect("parse stream chunk"),
        vec![AgentStreamEvent::MessageStop {
            stop_reason: None,
            usage: Some(TokenUsage {
                input_tokens: Some(13),
                output_tokens: Some(5),
                cache_creation_input_tokens: None,
                cache_read_input_tokens: Some(8),
            }),
        }]
    );
}
