use codex_agent_protocol::AgentMessage;
use codex_agent_protocol::AgentRequest;
use codex_agent_protocol::AgentStreamEvent;
use codex_agent_protocol::AgentTool;
use codex_agent_protocol::ContentBlock;
use codex_agent_protocol::ContentDelta;
use codex_agent_protocol::MessageRole;
use codex_agent_protocol::ReasoningConfig;
use codex_agent_protocol::RequestMetadata;
use codex_agent_protocol::StopReason;
use codex_agent_protocol::TokenUsage;
use codex_agent_protocol::ToolChoice;
use codex_agent_protocol::ToolResultContent;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::collections::BTreeMap;

use super::AnthropicMessagesOptions;
use super::parse_stream_event;
use super::to_messages_request;

#[test]
fn messages_request_maps_agent_ir_to_anthropic_shape() {
    let request = AgentRequest {
        model: "astral-large".to_string(),
        instructions: vec![ContentBlock::Text {
            text: "You are Astral-Code.".to_string(),
        }],
        messages: vec![
            AgentMessage {
                role: MessageRole::Developer,
                content: vec![ContentBlock::Text {
                    text: "Prefer concise tool calls.".to_string(),
                }],
                id: None,
            },
            AgentMessage {
                role: MessageRole::User,
                content: vec![ContentBlock::Text {
                    text: "list files".to_string(),
                }],
                id: Some("msg-user".to_string()),
            },
            AgentMessage {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "toolu_1".to_string(),
                    name: "Bash".to_string(),
                    input: json!({ "command": "ls" }),
                }],
                id: Some("msg-assistant".to_string()),
            },
            AgentMessage {
                role: MessageRole::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "toolu_1".to_string(),
                    content: vec![ToolResultContent::Json {
                        value: json!({ "stdout": "Cargo.toml" }),
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
        tool_choice: ToolChoice::Required,
        parallel_tool_calls: false,
        stream: true,
        reasoning: Some(ReasoningConfig {
            effort: Some("medium".to_string()),
            summary: None,
        }),
        metadata: RequestMetadata::default(),
    };

    assert_eq!(
        to_messages_request(&request, AnthropicMessagesOptions { max_tokens: 4096 }),
        json!({
            "model": "astral-large",
            "max_tokens": 4096,
            "stream": true,
            "system": [
                { "type": "text", "text": "You are Astral-Code." },
                { "type": "text", "text": "Prefer concise tool calls." }
            ],
            "messages": [
                {
                    "role": "user",
                    "content": [{ "type": "text", "text": "list files" }]
                },
                {
                    "role": "assistant",
                    "content": [{
                        "type": "tool_use",
                        "id": "toolu_1",
                        "name": "Bash",
                        "input": { "command": "ls" }
                    }]
                },
                {
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": "toolu_1",
                        "content": [{ "type": "text", "text": r#"{"stdout":"Cargo.toml"}"# }]
                    }]
                }
            ],
            "tools": [{
                "name": "Bash",
                "description": "Run a shell command",
                "input_schema": {
                    "type": "object",
                    "properties": { "command": { "type": "string" } },
                    "required": ["command"]
                }
            }],
            "tool_choice": { "type": "any" }
        })
    );
}

#[test]
fn stream_parser_maps_anthropic_events_to_agent_ir() {
    assert_eq!(
        parse_stream_event(json!({
            "type": "message_start",
            "message": { "id": "msg_1", "model": "astral-fast" }
        }))
        .expect("parse message_start"),
        Some(AgentStreamEvent::MessageStart {
            id: Some("msg_1".to_string()),
            model: Some("astral-fast".to_string()),
        })
    );

    assert_eq!(
        parse_stream_event(json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {
                "type": "tool_use",
                "id": "toolu_1",
                "name": "Bash",
                "input": {}
            }
        }))
        .expect("parse content_block_start"),
        Some(AgentStreamEvent::ContentBlockStart {
            index: 0,
            block: ContentBlock::ToolUse {
                id: "toolu_1".to_string(),
                name: "Bash".to_string(),
                input: json!({}),
            },
        })
    );

    assert_eq!(
        parse_stream_event(json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "input_json_delta", "partial_json": r#"{"command":"pwd"}"# }
        }))
        .expect("parse content_block_delta"),
        Some(AgentStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolInputJson {
                partial_json: r#"{"command":"pwd"}"#.to_string(),
            },
        })
    );

    assert_eq!(
        parse_stream_event(json!({
            "type": "message_delta",
            "delta": { "stop_reason": "tool_use" },
            "usage": { "output_tokens": 7, "cache_read_input_tokens": 11 }
        }))
        .expect("parse message_delta"),
        Some(AgentStreamEvent::MessageStop {
            stop_reason: Some(StopReason::ToolUse),
            usage: Some(TokenUsage {
                input_tokens: None,
                output_tokens: Some(7),
                cache_creation_input_tokens: None,
                cache_read_input_tokens: Some(11),
            }),
        })
    );

    assert_eq!(
        parse_stream_event(json!({ "type": "message_stop" })).expect("parse message_stop"),
        None
    );
}

#[test]
fn stream_parser_maps_error_event_to_terminal_error_reason() {
    assert_eq!(
        parse_stream_event(json!({
            "type": "error",
            "error": { "message": "rate limited" }
        }))
        .expect("parse error"),
        Some(AgentStreamEvent::MessageStop {
            stop_reason: Some(StopReason::Error {
                message: "rate limited".to_string(),
            }),
            usage: None,
        })
    );
}
