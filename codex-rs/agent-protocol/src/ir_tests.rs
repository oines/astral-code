use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

#[test]
fn agent_request_serializes_tool_use_and_tool_result_blocks() {
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
        parallel_tool_calls: false,
        stream: true,
        reasoning: Some(ReasoningConfig {
            effort: Some("medium".to_string()),
            summary: None,
        }),
        metadata: RequestMetadata {
            service_tier: None,
            prompt_cache_key: Some("astral:test".to_string()),
            response_format: None,
            provider: BTreeMap::new(),
        },
    };

    assert_eq!(
        serde_json::to_value(request).expect("serialize request"),
        json!({
            "model": "astral-large",
            "instructions": [{ "type": "text", "text": "You are Astral-Code." }],
            "messages": [
                {
                    "role": "user",
                    "content": [{ "type": "text", "text": "list files" }],
                    "id": "msg-user"
                },
                {
                    "role": "assistant",
                    "content": [{
                        "type": "tool_use",
                        "id": "toolu_1",
                        "name": "Bash",
                        "input": { "command": "ls" }
                    }],
                    "id": "msg-assistant"
                },
                {
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": "toolu_1",
                        "content": [{ "type": "text", "text": "Cargo.toml" }]
                    }]
                }
            ],
            "tools": [{
                "name": "Bash",
                "description": "Run a shell command",
                "inputSchema": {
                    "type": "object",
                    "properties": { "command": { "type": "string" } },
                    "required": ["command"]
                }
            }],
            "toolChoice": { "type": "auto" },
            "parallelToolCalls": false,
            "stream": true,
            "reasoning": { "effort": "medium" },
            "metadata": { "promptCacheKey": "astral:test" }
        })
    );
}

#[test]
fn stream_events_preserve_tool_json_deltas_and_usage() {
    let events = vec![
        AgentStreamEvent::MessageStart {
            id: Some("msg_1".to_string()),
            model: Some("astral-fast".to_string()),
            usage: None,
        },
        AgentStreamEvent::ContentBlockStart {
            index: 0,
            block: ContentBlock::ToolUse {
                id: "toolu_1".to_string(),
                name: "Bash".to_string(),
                input: json!({}),
            },
        },
        AgentStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolInputJson {
                partial_json: r#"{"command":"pwd"}"#.to_string(),
            },
        },
        AgentStreamEvent::MessageStop {
            stop_reason: Some(StopReason::ToolUse),
            usage: Some(TokenUsage {
                input_tokens: Some(10),
                output_tokens: Some(5),
                cache_creation_input_tokens: None,
                cache_read_input_tokens: Some(3),
            }),
        },
    ];

    assert_eq!(
        serde_json::to_value(events).expect("serialize stream events"),
        json!([
            {
                "type": "message_start",
                "id": "msg_1",
                "model": "astral-fast"
            },
            {
                "type": "content_block_start",
                "index": 0,
                "block": {
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": "Bash",
                    "input": {}
                }
            },
            {
                "type": "content_block_delta",
                "index": 0,
                "delta": {
                    "type": "tool_input_json",
                    "partial_json": r#"{"command":"pwd"}"#
                }
            },
            {
                "type": "message_stop",
                "stop_reason": { "type": "tool_use" },
                "usage": {
                    "inputTokens": 10,
                    "outputTokens": 5,
                    "cacheReadInputTokens": 3
                }
            }
        ])
    );
}
