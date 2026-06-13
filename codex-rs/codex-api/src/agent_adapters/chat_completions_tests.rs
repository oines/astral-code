use codex_agent_protocol::AgentMessage;
use codex_agent_protocol::AgentRequest;
use codex_agent_protocol::AgentStreamEvent;
use codex_agent_protocol::AgentTool;
use codex_agent_protocol::ContentBlock;
use codex_agent_protocol::ContentDelta;
use codex_agent_protocol::ImageSource;
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
                    "content": "list files"
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
fn request_collapses_system_and_developer_messages_to_head_system_message() {
    let request = AgentRequest {
        model: "astral-large".to_string(),
        instructions: vec![ContentBlock::Text {
            text: "Base instructions.".to_string(),
        }],
        messages: vec![
            AgentMessage {
                role: MessageRole::User,
                content: vec![ContentBlock::Text {
                    text: "hello".to_string(),
                }],
                id: None,
            },
            AgentMessage {
                role: MessageRole::Developer,
                content: vec![ContentBlock::Text {
                    text: "Developer instruction.".to_string(),
                }],
                id: None,
            },
            AgentMessage {
                role: MessageRole::System,
                content: vec![ContentBlock::Text {
                    text: "System reminder.".to_string(),
                }],
                id: None,
            },
        ],
        tools: Vec::new(),
        tool_choice: ToolChoice::Auto,
        parallel_tool_calls: true,
        stream: false,
        reasoning: None,
        metadata: RequestMetadata::default(),
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions { max_tokens: None }),
        json!({
            "model": "astral-large",
            "stream": false,
            "messages": [
                {
                    "role": "system",
                    "content": "Base instructions.\n\nDeveloper instruction.\n\nSystem reminder."
                },
                {
                    "role": "user",
                    "content": "hello"
                }
            ]
        })
    );
}

#[test]
fn request_drops_tool_control_fields_when_provider_override_clears_tools() {
    let request = AgentRequest {
        model: "astral-large".to_string(),
        instructions: Vec::new(),
        messages: vec![AgentMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::Text {
                text: "hello".to_string(),
            }],
            id: None,
        }],
        tools: vec![AgentTool {
            name: "Bash".to_string(),
            description: "Run a shell command".to_string(),
            input_schema: json!({ "type": "object" }),
            metadata: BTreeMap::new(),
        }],
        tool_choice: ToolChoice::Auto,
        parallel_tool_calls: true,
        stream: false,
        reasoning: None,
        metadata: RequestMetadata {
            provider: BTreeMap::from([("tools".to_string(), json!([]))]),
            ..RequestMetadata::default()
        },
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions { max_tokens: None }),
        json!({
            "model": "astral-large",
            "stream": false,
            "messages": [{
                "role": "user",
                "content": "hello"
            }],
            "tools": []
        })
    );
}

#[test]
fn request_provider_null_override_removes_default_field() {
    let request = AgentRequest {
        model: "strict-compatible".to_string(),
        messages: vec![AgentMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::Text {
                text: "hello".to_string(),
            }],
            id: None,
        }],
        stream: true,
        metadata: RequestMetadata {
            provider: BTreeMap::from([
                ("stream_options".to_string(), json!(null)),
                ("temperature".to_string(), json!(0.1)),
            ]),
            ..RequestMetadata::default()
        },
        ..AgentRequest::default()
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions { max_tokens: None }),
        json!({
            "model": "strict-compatible",
            "stream": true,
            "temperature": 0.1,
            "messages": [{
                "role": "user",
                "content": "hello"
            }]
        })
    );
}

#[test]
fn request_keeps_multimodal_user_content_as_parts_array() {
    let request = AgentRequest {
        model: "vision-compatible".to_string(),
        messages: vec![AgentMessage {
            role: MessageRole::User,
            content: vec![
                ContentBlock::Text {
                    text: "inspect".to_string(),
                },
                ContentBlock::Image {
                    source: ImageSource::Url {
                        url: "https://example.com/screenshot.png".to_string(),
                    },
                },
            ],
            id: None,
        }],
        stream: false,
        ..AgentRequest::default()
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions { max_tokens: None }),
        json!({
            "model": "vision-compatible",
            "stream": false,
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "inspect" },
                    {
                        "type": "image_url",
                        "image_url": { "url": "https://example.com/screenshot.png" }
                    }
                ]
            }]
        })
    );
}

#[test]
fn request_preserves_assistant_reasoning_content_for_deepseek() {
    let request = AgentRequest {
        model: "deepseek-v4-pro".to_string(),
        messages: vec![AgentMessage {
            role: MessageRole::Assistant,
            content: vec![
                ContentBlock::Reasoning {
                    text: "I should inspect the repo first.".to_string(),
                    signature: None,
                },
                ContentBlock::Text {
                    text: "I will check the files.".to_string(),
                },
            ],
            id: None,
        }],
        stream: false,
        ..AgentRequest::default()
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions { max_tokens: None }),
        json!({
            "model": "deepseek-v4-pro",
            "stream": false,
            "messages": [{
                "role": "assistant",
                "content": "I will check the files.",
                "reasoning_content": "I should inspect the repo first."
            }]
        })
    );
}

#[test]
fn request_sets_empty_content_for_reasoning_only_assistant_message() {
    let request = AgentRequest {
        model: "deepseek-v4-pro".to_string(),
        messages: vec![AgentMessage {
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Reasoning {
                text: "I should answer briefly.".to_string(),
                signature: None,
            }],
            id: None,
        }],
        stream: false,
        ..AgentRequest::default()
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions { max_tokens: None }),
        json!({
            "model": "deepseek-v4-pro",
            "stream": false,
            "messages": [{
                "role": "assistant",
                "content": "",
                "reasoning_content": "I should answer briefly."
            }]
        })
    );
}

#[test]
fn request_merges_adjacent_assistant_reasoning_and_tool_calls() {
    let request = AgentRequest {
        model: "deepseek-v4-pro".to_string(),
        messages: vec![
            AgentMessage {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Reasoning {
                    text: "I should inspect first.".to_string(),
                    signature: None,
                }],
                id: None,
            },
            AgentMessage {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "Bash".to_string(),
                    input: json!({ "command": "find . -name '*.py'" }),
                }],
                id: None,
            },
            AgentMessage {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "call_2".to_string(),
                    name: "Bash".to_string(),
                    input: json!({ "command": "python3 -m unittest -q" }),
                }],
                id: None,
            },
        ],
        stream: false,
        ..AgentRequest::default()
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions { max_tokens: None }),
        json!({
            "model": "deepseek-v4-pro",
            "stream": false,
            "messages": [{
                "role": "assistant",
                "content": "",
                "reasoning_content": "I should inspect first.",
                "tool_calls": [
                    {
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "Bash",
                            "arguments": r#"{"command":"find . -name '*.py'"}"#
                        }
                    },
                    {
                        "id": "call_2",
                        "type": "function",
                        "function": {
                            "name": "Bash",
                            "arguments": r#"{"command":"python3 -m unittest -q"}"#
                        }
                    }
                ]
            }]
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
                index: 2,
                block: ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "Bash".to_string(),
                    input: json!({}),
                },
            },
            AgentStreamEvent::ContentBlockDelta {
                index: 2,
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
fn stream_chunk_maps_deepseek_reasoning_content_delta() {
    assert_eq!(
        parse_stream_chunk(json!({
            "id": "chatcmpl_reasoning",
            "model": "deepseek-v4-pro",
            "choices": [{
                "delta": {
                    "role": "assistant",
                    "reasoning_content": "I should inspect the repo first."
                }
            }]
        }))
        .expect("parse stream chunk"),
        vec![
            AgentStreamEvent::MessageStart {
                id: Some("chatcmpl_reasoning".to_string()),
                model: Some("deepseek-v4-pro".to_string()),
            },
            AgentStreamEvent::ContentBlockDelta {
                index: 1,
                delta: ContentDelta::Reasoning {
                    text: "I should inspect the repo first.".to_string(),
                },
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
fn stream_chunk_ignores_null_usage() {
    assert_eq!(
        parse_stream_chunk(json!({
            "id": "chatcmpl_1",
            "model": "deepseek-v4-pro",
            "choices": [{
                "delta": { "role": "assistant", "content": null },
                "finish_reason": null
            }],
            "usage": null
        }))
        .expect("parse stream chunk"),
        vec![AgentStreamEvent::MessageStart {
            id: Some("chatcmpl_1".to_string()),
            model: Some("deepseek-v4-pro".to_string()),
        }]
    );
}

#[test]
fn stream_chunk_maps_finish_reason_without_delta() {
    assert_eq!(
        parse_stream_chunk(json!({
            "choices": [{ "finish_reason": "stop" }]
        }))
        .expect("parse stream chunk"),
        vec![AgentStreamEvent::MessageStop {
            stop_reason: Some(StopReason::EndTurn),
            usage: None,
        }]
    );
}

#[test]
fn stream_chunk_maps_error_payload_to_terminal_error_reason() {
    assert_eq!(
        parse_stream_chunk(json!({
            "error": { "message": "provider overloaded" }
        }))
        .expect("parse stream chunk"),
        vec![AgentStreamEvent::MessageStop {
            stop_reason: Some(StopReason::Error {
                message: "provider overloaded".to_string(),
            }),
            usage: None,
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

#[test]
fn stream_chunk_maps_deepseek_cache_usage_fields() {
    assert_eq!(
        parse_stream_chunk(json!({
            "choices": [],
            "usage": {
                "prompt_tokens": 23,
                "completion_tokens": 5,
                "prompt_cache_hit_tokens": 17,
                "prompt_cache_miss_tokens": 6,
                "total_tokens": 28
            }
        }))
        .expect("parse stream chunk"),
        vec![AgentStreamEvent::MessageStop {
            stop_reason: None,
            usage: Some(TokenUsage {
                input_tokens: Some(23),
                output_tokens: Some(5),
                cache_creation_input_tokens: None,
                cache_read_input_tokens: Some(17),
            }),
        }]
    );
}
