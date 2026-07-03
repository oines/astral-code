use codex_agent_protocol::AgentMessage;
use codex_agent_protocol::AgentRequest;
use codex_agent_protocol::AgentStreamEvent;
use codex_agent_protocol::AgentTool;
use codex_agent_protocol::ContentBlock;
use codex_agent_protocol::ContentDelta;
use codex_agent_protocol::ImageSource;
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

use super::AnthropicCacheFoldOptions;
use super::AnthropicMessagesOptions;
use super::AnthropicPinnedCacheEdits;
use super::parse_stream_event;
use super::to_messages_request;
use super::to_messages_request_parts;

fn options(max_tokens: u64) -> AnthropicMessagesOptions {
    AnthropicMessagesOptions {
        max_tokens,
        supports_image_input: true,
        cache_fold: None,
        compact_input_placeholders: false,
    }
}

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
        metadata: RequestMetadata {
            provider: BTreeMap::from([
                ("temperature".to_string(), json!(0.2)),
                ("top_p".to_string(), json!(0.9)),
            ]),
            ..RequestMetadata::default()
        },
    };

    assert_eq!(
        to_messages_request(&request, options(4096)),
        json!({
            "model": "astral-large",
            "max_tokens": 4096,
            "stream": true,
            "thinking": { "type": "enabled", "budget_tokens": 4095 },
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
            "tool_choice": { "type": "any" },
            "temperature": 0.2,
            "top_p": 0.9
        })
    );
}

#[test]
fn messages_request_adds_cache_control_when_prompt_cache_key_is_set() {
    let request = AgentRequest {
        model: "astral-large".to_string(),
        instructions: vec![ContentBlock::Text {
            text: "You are Astral-Code.".to_string(),
        }],
        messages: vec![AgentMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::Text {
                text: "inspect the repo".to_string(),
            }],
            id: None,
        }],
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
        metadata: RequestMetadata {
            prompt_cache_key: Some("astral:test".to_string()),
            ..RequestMetadata::default()
        },
        stream: true,
        ..AgentRequest::default()
    };

    assert_eq!(
        to_messages_request(&request, options(1024)),
        json!({
            "model": "astral-large",
            "max_tokens": 1024,
            "stream": true,
            "system": [{
                "type": "text",
                "text": "You are Astral-Code.",
                "cache_control": { "type": "ephemeral" }
            }],
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "text",
                    "text": "inspect the repo",
                    "cache_control": { "type": "ephemeral" }
                }]
            }],
            "tools": [{
                "name": "Bash",
                "description": "Run a shell command",
                "input_schema": {
                    "type": "object",
                    "properties": { "command": { "type": "string" } },
                    "required": ["command"]
                },
                "cache_control": { "type": "ephemeral" }
            }],
            "tool_choice": { "type": "auto" }
        })
    );
}

#[test]
fn messages_request_caches_compaction_summary_and_top_level_prefix() {
    let request = AgentRequest {
        model: "astral-large".to_string(),
        instructions: vec![ContentBlock::Text {
            text: "You are Astral-Code.".to_string(),
        }],
        messages: vec![AgentMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::Compaction {
                text: "Compacted summary".to_string(),
            }],
            id: None,
        }],
        metadata: RequestMetadata {
            prompt_cache_key: Some("astral:test".to_string()),
            ..RequestMetadata::default()
        },
        stream: true,
        ..AgentRequest::default()
    };

    assert_eq!(
        to_messages_request(&request, options(1024)),
        json!({
            "model": "astral-large",
            "max_tokens": 1024,
            "stream": true,
            "system": [{
                "type": "text",
                "text": "You are Astral-Code.",
                "cache_control": { "type": "ephemeral" }
            }],
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "text",
                    "text": "Compacted summary",
                    "cache_control": { "type": "ephemeral" }
                }]
            }]
        })
    );
}

#[test]
fn messages_request_cached_fold_is_only_added_from_options() {
    let messages = (1..=6)
        .flat_map(|index| {
            [
                AgentMessage {
                    role: MessageRole::Assistant,
                    content: vec![ContentBlock::ToolUse {
                        id: format!("toolu_{index}"),
                        name: "Read".to_string(),
                        input: json!({ "file_path": format!("file-{index}.txt") }),
                    }],
                    id: None,
                },
                AgentMessage {
                    role: MessageRole::User,
                    content: vec![ContentBlock::ToolResult {
                        tool_use_id: format!("toolu_{index}"),
                        content: vec![ToolResultContent::Text {
                            text: format!("file {index} contents"),
                        }],
                        is_error: false,
                    }],
                    id: None,
                },
            ]
        })
        .chain([AgentMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::Text {
                text: "continue".to_string(),
            }],
            id: None,
        }])
        .collect::<Vec<_>>();
    let request = AgentRequest {
        model: "astral-large".to_string(),
        instructions: vec![ContentBlock::Text {
            text: "You are Astral-Code.".to_string(),
        }],
        messages,
        metadata: RequestMetadata {
            prompt_cache_key: Some("astral:test".to_string()),
            ..RequestMetadata::default()
        },
        stream: true,
        ..AgentRequest::default()
    };

    let body_without_fold = to_messages_request(&request, options(1024));
    let body_without_fold = serde_json::to_string(&body_without_fold).expect("serialize body");
    assert!(!body_without_fold.contains("cache_edits"));
    assert!(!body_without_fold.contains("cache_reference"));
    assert!(!body_without_fold.contains("Old tool results may be automatically cleared"));

    let mut options = options(1024);
    options.cache_fold = Some(AnthropicCacheFoldOptions {
        cache_reference_tool_use_ids: (1..=6).map(|index| format!("toolu_{index}")).collect(),
        pinned_cache_edits: vec![AnthropicPinnedCacheEdits {
            user_message_index: 11,
            cache_references: vec!["toolu_1".to_string()],
        }],
    });

    let body_with_fold = to_messages_request(&request, options);
    assert_eq!(
        body_with_fold["messages"][1]["content"][0]["cache_reference"],
        "toolu_1"
    );
    assert_eq!(
        body_with_fold["messages"][11],
        json!({
            "role": "user",
            "content": [
                {
                    "type": "tool_result",
                    "tool_use_id": "toolu_6",
                    "content": [{ "type": "text", "text": "file 6 contents" }]
                },
                {
                    "type": "cache_edits",
                    "edits": [{
                        "type": "delete",
                        "cache_reference": "toolu_1"
                    }]
                },
                {
                    "type": "text",
                    "text": "continue",
                    "cache_control": { "type": "ephemeral" }
                }
            ]
        })
    );
    assert_eq!(
        body_with_fold["system"][1],
        json!({
            "type": "text",
            "text": "Old tool results may be automatically cleared from context to free up space. The 5 most recent eligible tool results are always kept. When a tool result contains information you may need later, write down the important details in your response.",
            "cache_control": { "type": "ephemeral" }
        })
    );
}

#[test]
fn messages_request_compact_placeholders_replace_media_and_large_tool_result_content() {
    let request = AgentRequest {
        model: "astral-large".to_string(),
        messages: vec![AgentMessage {
            role: MessageRole::User,
            content: vec![
                ContentBlock::Image {
                    source: ImageSource::Base64 {
                        media_type: "image/png".to_string(),
                        data: "abc123".to_string(),
                    },
                    detail: None,
                },
                ContentBlock::ToolResult {
                    tool_use_id: "toolu_1".to_string(),
                    content: vec![
                        ToolResultContent::Text {
                            text: "small output".to_string(),
                        },
                        ToolResultContent::Text {
                            text: "x".repeat(4097),
                        },
                        ToolResultContent::Image {
                            source: ImageSource::Url {
                                url: "https://example.com/image.png".to_string(),
                            },
                            detail: None,
                        },
                    ],
                    is_error: false,
                },
            ],
            id: None,
        }],
        stream: true,
        ..AgentRequest::default()
    };
    let mut options = options(1024);
    options.compact_input_placeholders = true;

    assert_eq!(
        to_messages_request(&request, options),
        json!({
            "model": "astral-large",
            "max_tokens": 1024,
            "stream": true,
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_1",
                        "content": [
                            { "type": "text", "text": "small output" },
                            { "type": "text", "text": "[Old tool result content cleared]" },
                            { "type": "text", "text": "[image]" }
                        ]
                    },
                    { "type": "text", "text": "[image]" }
                ]
            }]
        })
    );
}

#[test]
fn messages_request_puts_tool_results_first_and_keeps_images_native() {
    let request = AgentRequest {
        model: "astral-large".to_string(),
        messages: vec![AgentMessage {
            role: MessageRole::User,
            content: vec![
                ContentBlock::Text {
                    text: "follow-up text".to_string(),
                },
                ContentBlock::ToolResult {
                    tool_use_id: "toolu_1".to_string(),
                    content: vec![
                        ToolResultContent::Text {
                            text: "metadata: image/png".to_string(),
                        },
                        ToolResultContent::Image {
                            source: ImageSource::Base64 {
                                media_type: "image/png".to_string(),
                                data: "abc123".to_string(),
                            },
                            detail: Some("original".to_string()),
                        },
                    ],
                    is_error: false,
                },
            ],
            id: None,
        }],
        stream: true,
        ..AgentRequest::default()
    };

    assert_eq!(
        to_messages_request(&request, options(1024)),
        json!({
            "model": "astral-large",
            "max_tokens": 1024,
            "stream": true,
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_1",
                        "content": [
                            { "type": "text", "text": "metadata: image/png" },
                            {
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": "image/png",
                                    "data": "abc123"
                                }
                            }
                        ]
                    },
                    { "type": "text", "text": "follow-up text" }
                ]
            }]
        })
    );
}

#[test]
fn messages_request_aliases_anthropic_incompatible_tool_names() {
    let long_name = "mcp__very_long_server_name_that_would_exceed_anthropic_tool_name_length__very_long_tool_name";
    let request = AgentRequest {
        model: "astral-large".to_string(),
        messages: vec![AgentMessage {
            role: MessageRole::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "toolu_1".to_string(),
                name: long_name.to_string(),
                input: json!({ "query": "hello" }),
            }],
            id: None,
        }],
        tools: vec![AgentTool {
            name: long_name.to_string(),
            description: "A long MCP tool".to_string(),
            input_schema: json!({ "type": "object" }),
            metadata: BTreeMap::from([
                ("deferLoading".to_string(), json!(true)),
                ("strict".to_string(), json!(true)),
            ]),
        }],
        tool_choice: ToolChoice::Tool {
            name: long_name.to_string(),
        },
        stream: true,
        ..AgentRequest::default()
    };

    let request = to_messages_request_parts(&request, options(1024));
    let alias = request
        .tool_name_aliases
        .iter()
        .find_map(|(alias, canonical)| (canonical == long_name).then_some(alias.clone()))
        .expect("long tool should be aliased");
    assert!(alias.len() <= 64);
    assert!(
        alias
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    );

    assert_eq!(
        request.body,
        json!({
            "model": "astral-large",
            "max_tokens": 1024,
            "stream": true,
            "messages": [{
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": alias,
                    "input": { "query": "hello" }
                }]
            }],
            "tools": [{
                "name": alias,
                "description": "A long MCP tool",
                "input_schema": { "type": "object" },
                "defer_loading": true,
                "strict": true
            }],
            "tool_choice": { "type": "tool", "name": alias }
        })
    );
}

#[test]
fn messages_request_provider_null_override_removes_default_field() {
    let request = AgentRequest {
        model: "anthropic-compatible".to_string(),
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
                ("stream".to_string(), json!(null)),
                ("temperature".to_string(), json!(0.1)),
            ]),
            ..RequestMetadata::default()
        },
        ..AgentRequest::default()
    };

    assert_eq!(
        to_messages_request(&request, options(1024)),
        json!({
            "model": "anthropic-compatible",
            "max_tokens": 1024,
            "temperature": 0.1,
            "messages": [{
                "role": "user",
                "content": [{ "type": "text", "text": "hello" }]
            }]
        })
    );
}

#[test]
fn messages_request_omits_tool_choice_without_tools() {
    let request = AgentRequest {
        model: "anthropic-compatible".to_string(),
        messages: vec![AgentMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::Text {
                text: "hello".to_string(),
            }],
            id: None,
        }],
        stream: true,
        ..AgentRequest::default()
    };

    assert_eq!(
        to_messages_request(&request, options(1024)),
        json!({
            "model": "anthropic-compatible",
            "max_tokens": 1024,
            "stream": true,
            "messages": [{
                "role": "user",
                "content": [{ "type": "text", "text": "hello" }]
            }]
        })
    );
}

#[test]
fn messages_request_merges_adjacent_reasoning_tool_use_and_tool_results() {
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
                    id: "toolu_1".to_string(),
                    name: "Bash".to_string(),
                    input: json!({ "command": "find . -name '*.py'" }),
                }],
                id: None,
            },
            AgentMessage {
                role: MessageRole::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "toolu_1".to_string(),
                    content: vec![ToolResultContent::Text {
                        text: "calculator.py".to_string(),
                    }],
                    is_error: false,
                }],
                id: None,
            },
            AgentMessage {
                role: MessageRole::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "toolu_2".to_string(),
                    content: vec![ToolResultContent::Text {
                        text: "test_calculator.py".to_string(),
                    }],
                    is_error: false,
                }],
                id: None,
            },
        ],
        stream: false,
        ..AgentRequest::default()
    };

    assert_eq!(
        to_messages_request(&request, options(4096)),
        json!({
            "model": "deepseek-v4-pro",
            "max_tokens": 4096,
            "stream": false,
            "messages": [
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool_use",
                            "id": "toolu_1",
                            "name": "Bash",
                            "input": { "command": "find . -name '*.py'" }
                        }
                    ]
                },
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "tool_result",
                            "tool_use_id": "toolu_1",
                            "content": [{ "type": "text", "text": "calculator.py" }]
                        },
                        {
                            "type": "tool_result",
                            "tool_use_id": "toolu_2",
                            "content": [{ "type": "text", "text": "test_calculator.py" }]
                        }
                    ]
                }
            ]
        })
    );
}

#[test]
fn messages_request_projects_signed_reasoning_only_when_thinking_enabled() {
    let request = AgentRequest {
        model: "anthropic-compatible".to_string(),
        messages: vec![AgentMessage {
            role: MessageRole::Assistant,
            content: vec![
                ContentBlock::Reasoning {
                    text: "I should inspect first.".to_string(),
                    signature: Some("sig_opaque".to_string()),
                },
                ContentBlock::Text {
                    text: "I found the file.".to_string(),
                },
            ],
            id: None,
        }],
        stream: false,
        reasoning: Some(ReasoningConfig {
            effort: Some("low".to_string()),
            summary: None,
        }),
        ..AgentRequest::default()
    };

    assert_eq!(
        to_messages_request(&request, options(4096)),
        json!({
            "model": "anthropic-compatible",
            "max_tokens": 4096,
            "stream": false,
            "thinking": { "type": "enabled", "budget_tokens": 1024 },
            "messages": [{
                "role": "assistant",
                "content": [
                    {
                        "type": "thinking",
                        "thinking": "I should inspect first.",
                        "signature": "sig_opaque"
                    },
                    { "type": "text", "text": "I found the file." }
                ]
            }]
        })
    );
}

#[test]
fn messages_request_drops_unsigned_reasoning_even_when_thinking_enabled() {
    let request = AgentRequest {
        model: "anthropic-compatible".to_string(),
        messages: vec![AgentMessage {
            role: MessageRole::Assistant,
            content: vec![
                ContentBlock::Reasoning {
                    text: "I should inspect first.".to_string(),
                    signature: None,
                },
                ContentBlock::Text {
                    text: "I found the file.".to_string(),
                },
            ],
            id: None,
        }],
        stream: false,
        reasoning: Some(ReasoningConfig {
            effort: Some("low".to_string()),
            summary: None,
        }),
        ..AgentRequest::default()
    };

    assert_eq!(
        to_messages_request(&request, options(4096)),
        json!({
            "model": "anthropic-compatible",
            "max_tokens": 4096,
            "stream": false,
            "thinking": { "type": "enabled", "budget_tokens": 1024 },
            "messages": [{
                "role": "assistant",
                "content": [{ "type": "text", "text": "I found the file." }]
            }]
        })
    );
}

#[test]
fn stream_parser_maps_anthropic_events_to_agent_ir() {
    assert_eq!(
        parse_stream_event(json!({
            "type": "message_start",
            "message": {
                "id": "msg_1",
                "model": "astral-fast",
                "usage": { "input_tokens": 19, "cache_creation_input_tokens": 5 }
            }
        }))
        .expect("parse message_start"),
        Some(AgentStreamEvent::MessageStart {
            id: Some("msg_1".to_string()),
            model: Some("astral-fast".to_string()),
            usage: Some(TokenUsage {
                input_tokens: Some(19),
                output_tokens: None,
                cache_creation_input_tokens: Some(5),
                cache_read_input_tokens: None,
            }),
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
            "type": "content_block_delta",
            "index": 1,
            "delta": { "type": "signature_delta", "signature": "sig_opaque" }
        }))
        .expect("parse signature_delta"),
        Some(AgentStreamEvent::ContentBlockDelta {
            index: 1,
            delta: ContentDelta::ReasoningSignature {
                signature: "sig_opaque".to_string(),
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

#[test]
fn stream_parser_skips_unknown_event_types() {
    assert_eq!(
        parse_stream_event(json!({
            "type": "message_metadata",
            "metadata": { "provider": "compatible-anthropic" }
        }))
        .expect("unknown event should not fail"),
        None
    );
}

#[test]
fn stream_parser_skips_unknown_content_blocks() {
    assert_eq!(
        parse_stream_event(json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {
                "type": "server_tool_use",
                "id": "srv_1",
                "name": "web_search"
            }
        }))
        .expect("unknown content block should not fail"),
        None
    );
}

#[test]
fn stream_parser_skips_redacted_thinking_blocks() {
    assert_eq!(
        parse_stream_event(json!({
            "type": "content_block_start",
            "index": 1,
            "content_block": {
                "type": "redacted_thinking",
                "data": "opaque"
            }
        }))
        .expect("redacted thinking should not fail"),
        None
    );
}

#[test]
fn stream_parser_skips_unknown_content_deltas() {
    assert_eq!(
        parse_stream_event(json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {
                "type": "citation_delta",
                "citation": { "url": "https://example.test" }
            }
        }))
        .expect("unknown content delta should not fail"),
        None
    );
}
