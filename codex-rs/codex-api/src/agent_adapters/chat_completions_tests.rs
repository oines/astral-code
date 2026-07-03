use codex_agent_protocol::AgentMessage;
use codex_agent_protocol::AgentRequest;
use codex_agent_protocol::AgentStreamEvent;
use codex_agent_protocol::AgentTool;
use codex_agent_protocol::ContentBlock;
use codex_agent_protocol::ContentDelta;
use codex_agent_protocol::ImageSource;
use codex_agent_protocol::MessageRole;
use codex_agent_protocol::PROVIDER_FLAVOR_METADATA_KEY;
use codex_agent_protocol::ReasoningConfig;
use codex_agent_protocol::RequestMetadata;
use codex_agent_protocol::StopReason;
use codex_agent_protocol::TokenUsage;
use codex_agent_protocol::ToolChoice;
use codex_agent_protocol::ToolResultContent;
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;

use super::super::CHAT_REASONING_CONTENT_METADATA_KEY;
use super::ChatCompletionsOptions;
use super::ChatCompletionsStreamState;
use super::parse_stream_chunk;
use super::parse_stream_chunk_with_state;
use super::to_chat_completions_request;

const ONE_BY_ONE_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";

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
                max_tokens: Some(1024),
                supports_image_input: true,
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
fn request_moves_image_tool_result_to_user_multimodal_message() {
    let request = AgentRequest {
        model: "mimo-v2.5".to_string(),
        instructions: Vec::new(),
        messages: vec![
            AgentMessage {
                role: MessageRole::User,
                content: vec![ContentBlock::Text {
                    text: "inspect image".to_string(),
                }],
                id: None,
            },
            AgentMessage {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "call_view".to_string(),
                    name: "view_image".to_string(),
                    input: json!({ "path": "/tmp/test.png" }),
                }],
                id: None,
            },
            AgentMessage {
                role: MessageRole::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call_view".to_string(),
                    content: vec![
                        ToolResultContent::Text {
                            text: "metadata: 64x32 image/png".to_string(),
                        },
                        ToolResultContent::Image {
                            source: ImageSource::Base64 {
                                media_type: "image/png".to_string(),
                                data: ONE_BY_ONE_PNG_BASE64.to_string(),
                            },
                            detail: Some("high".to_string()),
                        },
                    ],
                    is_error: false,
                }],
                id: None,
            },
        ],
        tools: vec![AgentTool {
            name: "view_image".to_string(),
            description: "View a local image".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }),
            metadata: BTreeMap::new(),
        }],
        tool_choice: ToolChoice::Auto,
        parallel_tool_calls: true,
        stream: false,
        reasoning: None,
        metadata: RequestMetadata::default(),
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions::default()),
        json!({
            "model": "mimo-v2.5",
            "stream": false,
            "messages": [
                { "role": "user", "content": "inspect image" },
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_view",
                        "type": "function",
                        "function": {
                            "name": "view_image",
                            "arguments": r#"{"path":"/tmp/test.png"}"#
                        }
                    }]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_view",
                    "content": "metadata: 64x32 image/png\n\nTool returned image content. The image is attached in the following user message."
                },
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "Image returned by view_image tool call call_view."
                        },
                        {
                            "type": "image_url",
                            "image_url": {
                                "url": format!("data:image/png;base64,{ONE_BY_ONE_PNG_BASE64}"),
                                "detail": "high"
                            }
                        }
                    ]
                }
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "view_image",
                    "description": "View a local image",
                    "parameters": {
                        "type": "object",
                        "properties": { "path": { "type": "string" } },
                        "required": ["path"]
                    }
                }
            }],
            "tool_choice": "auto",
            "parallel_tool_calls": true
        })
    );
}

#[test]
fn request_appends_image_user_messages_after_all_tool_results() {
    let request = AgentRequest {
        model: "mimo-v2.5".to_string(),
        messages: vec![
            AgentMessage {
                role: MessageRole::Assistant,
                content: vec![
                    ContentBlock::ToolUse {
                        id: "call_image".to_string(),
                        name: "view_image".to_string(),
                        input: json!({ "path": "/tmp/test.png" }),
                    },
                    ContentBlock::ToolUse {
                        id: "call_text".to_string(),
                        name: "Bash".to_string(),
                        input: json!({ "command": "pwd" }),
                    },
                ],
                id: None,
            },
            AgentMessage {
                role: MessageRole::User,
                content: vec![
                    ContentBlock::ToolResult {
                        tool_use_id: "call_image".to_string(),
                        content: vec![ToolResultContent::Image {
                            source: ImageSource::Base64 {
                                media_type: "image/png".to_string(),
                                data: ONE_BY_ONE_PNG_BASE64.to_string(),
                            },
                            detail: None,
                        }],
                        is_error: false,
                    },
                    ContentBlock::ToolResult {
                        tool_use_id: "call_text".to_string(),
                        content: vec![ToolResultContent::Text {
                            text: "/tmp".to_string(),
                        }],
                        is_error: false,
                    },
                ],
                id: None,
            },
        ],
        stream: false,
        ..AgentRequest::default()
    };

    let request = to_chat_completions_request(&request, ChatCompletionsOptions::default());
    let messages = request["messages"]
        .as_array()
        .expect("messages should be an array");

    assert_eq!(messages[0]["role"], "assistant");
    assert_eq!(messages[1]["role"], "tool");
    assert_eq!(messages[1]["tool_call_id"], "call_image");
    assert_eq!(messages[2]["role"], "tool");
    assert_eq!(messages[2]["tool_call_id"], "call_text");
    assert_eq!(messages[3]["role"], "user");
}

#[test]
fn request_omits_image_tool_result_for_text_only_model() {
    let request = AgentRequest {
        model: "mimo-v2.5".to_string(),
        messages: vec![
            AgentMessage {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "call_view".to_string(),
                    name: "view_image".to_string(),
                    input: json!({ "path": "/tmp/test.png" }),
                }],
                id: None,
            },
            AgentMessage {
                role: MessageRole::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call_view".to_string(),
                    content: vec![
                        ToolResultContent::Text {
                            text: "metadata: image/png".to_string(),
                        },
                        ToolResultContent::Image {
                            source: ImageSource::Url {
                                url: "https://example.com/test.png".to_string(),
                            },
                            detail: Some("high".to_string()),
                        },
                    ],
                    is_error: false,
                }],
                id: None,
            },
        ],
        stream: false,
        ..AgentRequest::default()
    };

    assert_eq!(
        to_chat_completions_request(
            &request,
            ChatCompletionsOptions {
                max_tokens: None,
                supports_image_input: false,
            }
        ),
        json!({
            "model": "mimo-v2.5",
            "stream": false,
            "messages": [
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_view",
                        "type": "function",
                        "function": {
                            "name": "view_image",
                            "arguments": r#"{"path":"/tmp/test.png"}"#
                        }
                    }]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_view",
                    "content": "metadata: image/png\n\n<image content omitted because you do not support image input>"
                }
            ]
        })
    );
}

#[test]
fn request_preserves_tool_result_image_data_urls_without_processing() {
    let request = AgentRequest {
        model: "vision-compatible".to_string(),
        messages: vec![
            AgentMessage {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "call_view".to_string(),
                    name: "view_image".to_string(),
                    input: json!({ "path": "/tmp/bad.png" }),
                }],
                id: None,
            },
            AgentMessage {
                role: MessageRole::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call_view".to_string(),
                    content: vec![ToolResultContent::Image {
                        source: ImageSource::Base64 {
                            media_type: "image/png".to_string(),
                            data: "not base64".to_string(),
                        },
                        detail: Some("high".to_string()),
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
        to_chat_completions_request(&request, ChatCompletionsOptions::default()),
        json!({
            "model": "vision-compatible",
            "stream": false,
            "messages": [
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_view",
                        "type": "function",
                        "function": {
                            "name": "view_image",
                            "arguments": r#"{"path":"/tmp/bad.png"}"#
                        }
                    }]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_view",
                    "content": "Tool returned image content. The image is attached in the following user message."
                },
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "Image returned by view_image tool call call_view."
                        },
                        {
                            "type": "image_url",
                            "image_url": {
                                "url": "data:image/png;base64,not base64",
                                "detail": "high"
                            }
                        }
                    ]
                }
            ]
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
        to_chat_completions_request(&request, ChatCompletionsOptions::default()),
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
fn request_maps_response_format_metadata() {
    let request = AgentRequest {
        model: "astral-large".to_string(),
        metadata: RequestMetadata {
            response_format: Some(json!({
                "type": "json_schema",
                "json_schema": {
                    "name": "codex_output_schema",
                    "strict": true,
                    "schema": {
                        "type": "object",
                        "properties": {
                            "answer": { "type": "string" }
                        },
                        "required": ["answer"],
                        "additionalProperties": false
                    }
                }
            })),
            ..RequestMetadata::default()
        },
        ..AgentRequest::default()
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions::default()),
        json!({
            "model": "astral-large",
            "stream": true,
            "stream_options": { "include_usage": true },
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "codex_output_schema",
                    "strict": true,
                    "schema": {
                        "type": "object",
                        "properties": {
                            "answer": { "type": "string" }
                        },
                        "required": ["answer"],
                        "additionalProperties": false
                    }
                }
            },
            "messages": []
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
        to_chat_completions_request(&request, ChatCompletionsOptions::default()),
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
        to_chat_completions_request(&request, ChatCompletionsOptions::default()),
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
fn request_applies_deepseek_reasoning_shape() {
    let request = AgentRequest {
        model: "deepseek-v4-pro".to_string(),
        stream: false,
        reasoning: Some(ReasoningConfig {
            effort: Some("xhigh".to_string()),
            summary: None,
        }),
        metadata: RequestMetadata {
            provider: BTreeMap::from([(
                PROVIDER_FLAVOR_METADATA_KEY.to_string(),
                json!("deepseek"),
            )]),
            ..RequestMetadata::default()
        },
        ..AgentRequest::default()
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions::default()),
        json!({
            "model": "deepseek-v4-pro",
            "stream": false,
            "messages": [],
            "thinking": { "type": "enabled" },
            "reasoning_effort": "max"
        })
    );
}

#[test]
fn request_applies_enable_thinking_shape() {
    let request = AgentRequest {
        model: "qwen3-coder".to_string(),
        stream: false,
        reasoning: Some(ReasoningConfig {
            effort: Some("none".to_string()),
            summary: None,
        }),
        metadata: RequestMetadata {
            provider: BTreeMap::from([(
                PROVIDER_FLAVOR_METADATA_KEY.to_string(),
                json!("enable_thinking"),
            )]),
            ..RequestMetadata::default()
        },
        ..AgentRequest::default()
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions::default()),
        json!({
            "model": "qwen3-coder",
            "stream": false,
            "messages": [],
            "enable_thinking": false
        })
    );
}

#[test]
fn request_applies_thinking_type_shape() {
    let request = AgentRequest {
        model: "glm-5.1".to_string(),
        stream: false,
        reasoning: Some(ReasoningConfig {
            effort: Some("high".to_string()),
            summary: None,
        }),
        metadata: RequestMetadata {
            provider: BTreeMap::from([(
                PROVIDER_FLAVOR_METADATA_KEY.to_string(),
                json!("thinking_type"),
            )]),
            ..RequestMetadata::default()
        },
        ..AgentRequest::default()
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions::default()),
        json!({
            "model": "glm-5.1",
            "stream": false,
            "messages": [],
            "thinking": { "type": "enabled" }
        })
    );
}

#[test]
fn request_applies_minimax_reasoning_shape() {
    let request = AgentRequest {
        model: "MiniMax-M2".to_string(),
        stream: false,
        reasoning: Some(ReasoningConfig {
            effort: Some("high".to_string()),
            summary: None,
        }),
        metadata: RequestMetadata {
            provider: BTreeMap::from([(
                PROVIDER_FLAVOR_METADATA_KEY.to_string(),
                json!("minimax"),
            )]),
            ..RequestMetadata::default()
        },
        ..AgentRequest::default()
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions::default()),
        json!({
            "model": "MiniMax-M2",
            "stream": false,
            "messages": [],
            "thinking": { "type": "enabled" },
            "reasoning_split": true
        })
    );
}

#[test]
fn request_applies_openrouter_reasoning_shape() {
    let request = AgentRequest {
        model: "deepseek/deepseek-chat-v3.1".to_string(),
        stream: false,
        reasoning: Some(ReasoningConfig {
            effort: Some("high".to_string()),
            summary: None,
        }),
        metadata: RequestMetadata {
            provider: BTreeMap::from([(
                PROVIDER_FLAVOR_METADATA_KEY.to_string(),
                json!("openrouter"),
            )]),
            ..RequestMetadata::default()
        },
        ..AgentRequest::default()
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions::default()),
        json!({
            "model": "deepseek/deepseek-chat-v3.1",
            "stream": false,
            "messages": [],
            "reasoning": { "effort": "high" }
        })
    );
}

#[test]
fn request_keeps_generic_openai_reasoning_private_fields_off() {
    let request = AgentRequest {
        model: "compatible-model".to_string(),
        stream: false,
        reasoning: Some(ReasoningConfig {
            effort: Some("high".to_string()),
            summary: None,
        }),
        metadata: RequestMetadata {
            provider: BTreeMap::from([(
                PROVIDER_FLAVOR_METADATA_KEY.to_string(),
                json!("generic_openai"),
            )]),
            ..RequestMetadata::default()
        },
        ..AgentRequest::default()
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions::default()),
        json!({
            "model": "compatible-model",
            "stream": false,
            "messages": []
        })
    );
}

#[test]
fn request_provider_override_can_clear_flavor_defaults() {
    let request = AgentRequest {
        model: "deepseek-v4-pro".to_string(),
        stream: false,
        reasoning: Some(ReasoningConfig {
            effort: Some("high".to_string()),
            summary: None,
        }),
        metadata: RequestMetadata {
            provider: BTreeMap::from([
                (PROVIDER_FLAVOR_METADATA_KEY.to_string(), json!("deepseek")),
                ("thinking".to_string(), json!(null)),
                ("reasoning_effort".to_string(), json!(null)),
            ]),
            ..RequestMetadata::default()
        },
        ..AgentRequest::default()
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions::default()),
        json!({
            "model": "deepseek-v4-pro",
            "stream": false,
            "messages": []
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
                    detail: None,
                },
            ],
            id: None,
        }],
        stream: false,
        metadata: RequestMetadata {
            provider: BTreeMap::from([(
                PROVIDER_FLAVOR_METADATA_KEY.to_string(),
                json!("deepseek"),
            )]),
            ..RequestMetadata::default()
        },
        ..AgentRequest::default()
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions::default()),
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
fn request_omits_user_image_for_text_only_model() {
    let request = AgentRequest {
        model: "text-only".to_string(),
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
                    detail: None,
                },
            ],
            id: None,
        }],
        stream: false,
        metadata: RequestMetadata {
            provider: BTreeMap::from([(
                PROVIDER_FLAVOR_METADATA_KEY.to_string(),
                json!("deepseek"),
            )]),
            ..RequestMetadata::default()
        },
        ..AgentRequest::default()
    };

    assert_eq!(
        to_chat_completions_request(
            &request,
            ChatCompletionsOptions {
                max_tokens: None,
                supports_image_input: false,
            }
        ),
        json!({
            "model": "text-only",
            "stream": false,
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "inspect" },
                    {
                        "type": "text",
                        "text": "<image content omitted because you do not support image input>"
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
        metadata: RequestMetadata {
            provider: BTreeMap::from([(
                PROVIDER_FLAVOR_METADATA_KEY.to_string(),
                json!("deepseek"),
            )]),
            ..RequestMetadata::default()
        },
        ..AgentRequest::default()
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions::default()),
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
fn request_omits_assistant_reasoning_content_for_generic_openai() {
    let request = AgentRequest {
        model: "compatible-model".to_string(),
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
        metadata: RequestMetadata {
            provider: BTreeMap::from([(
                PROVIDER_FLAVOR_METADATA_KEY.to_string(),
                json!("generic_openai"),
            )]),
            ..RequestMetadata::default()
        },
        ..AgentRequest::default()
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions::default()),
        json!({
            "model": "compatible-model",
            "stream": false,
            "messages": [{
                "role": "assistant",
                "content": "I will check the files."
            }]
        })
    );
}

#[test]
fn request_preserves_assistant_reasoning_content_when_metadata_enables_it() {
    let request = AgentRequest {
        model: "compatible-model".to_string(),
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
        metadata: RequestMetadata {
            provider: BTreeMap::from([
                (
                    PROVIDER_FLAVOR_METADATA_KEY.to_string(),
                    json!("generic_openai"),
                ),
                (CHAT_REASONING_CONTENT_METADATA_KEY.to_string(), json!(true)),
            ]),
            ..RequestMetadata::default()
        },
        ..AgentRequest::default()
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions::default()),
        json!({
            "model": "compatible-model",
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
        metadata: RequestMetadata {
            provider: BTreeMap::from([(
                PROVIDER_FLAVOR_METADATA_KEY.to_string(),
                json!("deepseek"),
            )]),
            ..RequestMetadata::default()
        },
        ..AgentRequest::default()
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions::default()),
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
        metadata: RequestMetadata {
            provider: BTreeMap::from([(
                PROVIDER_FLAVOR_METADATA_KEY.to_string(),
                json!("deepseek"),
            )]),
            ..RequestMetadata::default()
        },
        ..AgentRequest::default()
    };

    assert_eq!(
        to_chat_completions_request(&request, ChatCompletionsOptions::default()),
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
                usage: None,
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
                    input_tokens: Some(9),
                    output_tokens: Some(8),
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: Some(3),
                }),
            },
        ]
    );
}

#[test]
fn stream_chunk_assigns_missing_tool_call_indexes_by_id_order() {
    let mut state = ChatCompletionsStreamState::default();

    assert_eq!(
        parse_stream_chunk_with_state(
            json!({
                "choices": [{
                    "delta": {
                        "tool_calls": [
                            {
                                "id": "call_1",
                                "type": "function",
                                "function": { "name": "Bash", "arguments": "" }
                            },
                            {
                                "id": "call_2",
                                "type": "function",
                                "function": { "name": "Read", "arguments": "" }
                            }
                        ]
                    }
                }]
            }),
            &mut state,
        )
        .expect("parse first chunk"),
        vec![
            AgentStreamEvent::ContentBlockStart {
                index: 2,
                block: ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "Bash".to_string(),
                    input: json!({}),
                },
            },
            AgentStreamEvent::ContentBlockStart {
                index: 3,
                block: ContentBlock::ToolUse {
                    id: "call_2".to_string(),
                    name: "Read".to_string(),
                    input: json!({}),
                },
            },
        ]
    );

    assert_eq!(
        parse_stream_chunk_with_state(
            json!({
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "id": "call_2",
                            "type": "function",
                            "function": { "arguments": r#"{"file_path":"README.md"}"# }
                        }]
                    }
                }]
            }),
            &mut state,
        )
        .expect("parse second chunk"),
        vec![AgentStreamEvent::ContentBlockDelta {
            index: 3,
            delta: ContentDelta::ToolInputJson {
                partial_json: r#"{"file_path":"README.md"}"#.to_string(),
            },
        }]
    );
}

#[test]
fn stream_chunk_does_not_repeat_message_start_for_repeated_assistant_role() {
    let mut state = ChatCompletionsStreamState::default();

    assert_eq!(
        parse_stream_chunk_with_state(
            json!({
                "id": "chatcmpl_1",
                "model": "astral-fast",
                "choices": [{ "delta": { "role": "assistant" } }]
            }),
            &mut state,
        )
        .expect("parse first role chunk"),
        vec![AgentStreamEvent::MessageStart {
            id: Some("chatcmpl_1".to_string()),
            model: Some("astral-fast".to_string()),
            usage: None,
        }]
    );

    assert_eq!(
        parse_stream_chunk_with_state(
            json!({
                "id": "chatcmpl_1",
                "model": "astral-fast",
                "choices": [{ "delta": { "role": "assistant", "content": "hi" } }]
            }),
            &mut state,
        )
        .expect("parse repeated role chunk"),
        vec![AgentStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text {
                text: "hi".to_string(),
            },
        }]
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
                usage: None,
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
fn stream_chunk_maps_reasoning_details_delta() {
    assert_eq!(
        parse_stream_chunk(json!({
            "id": "chatcmpl_reasoning_details",
            "model": "MiniMax-M2",
            "choices": [{
                "delta": {
                    "role": "assistant",
                    "reasoning_details": [
                        { "text": "I should inspect first." },
                        { "content": "Then patch the file." },
                        "Finally rerun the test."
                    ]
                }
            }]
        }))
        .expect("parse stream chunk"),
        vec![
            AgentStreamEvent::MessageStart {
                id: Some("chatcmpl_reasoning_details".to_string()),
                model: Some("MiniMax-M2".to_string()),
                usage: None,
            },
            AgentStreamEvent::ContentBlockDelta {
                index: 1,
                delta: ContentDelta::Reasoning {
                    text: "I should inspect first.".to_string(),
                },
            },
            AgentStreamEvent::ContentBlockDelta {
                index: 1,
                delta: ContentDelta::Reasoning {
                    text: "Then patch the file.".to_string(),
                },
            },
            AgentStreamEvent::ContentBlockDelta {
                index: 1,
                delta: ContentDelta::Reasoning {
                    text: "Finally rerun the test.".to_string(),
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
fn stream_chunk_maps_text_and_usage_without_finish_as_usage_update() {
    assert_eq!(
        parse_stream_chunk(json!({
            "id": "chatcmpl_1",
            "model": "astral-fast",
            "choices": [{
                "delta": { "role": "assistant", "content": "hello" },
                "finish_reason": null
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 2 }
        }))
        .expect("parse stream chunk"),
        vec![
            AgentStreamEvent::MessageStart {
                id: Some("chatcmpl_1".to_string()),
                model: Some("astral-fast".to_string()),
                usage: None,
            },
            AgentStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text {
                    text: "hello".to_string(),
                },
            },
            AgentStreamEvent::MessageStop {
                stop_reason: None,
                usage: Some(TokenUsage {
                    input_tokens: Some(1),
                    output_tokens: Some(2),
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                }),
            },
        ]
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
            usage: None,
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
                input_tokens: Some(5),
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
                input_tokens: Some(6),
                output_tokens: Some(5),
                cache_creation_input_tokens: None,
                cache_read_input_tokens: Some(17),
            }),
        }]
    );
}

fn chat_text_wire_chunk(id: &str, model: &str, text: &str) -> Value {
    json!({
        "id": id,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {
                "role": "assistant",
                "content": text
            },
            "finish_reason": null
        }]
    })
}

fn chat_text_wire_chunk_from_events(events: &[AgentStreamEvent]) -> Value {
    let mut id = None;
    let mut model = None;
    let mut text = String::new();
    for event in events {
        match event {
            AgentStreamEvent::MessageStart {
                id: event_id,
                model: event_model,
                usage: None,
            } => {
                id = event_id.as_deref();
                model = event_model.as_deref();
            }
            AgentStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text { text: delta_text },
            } => text.push_str(delta_text),
            _ => {}
        }
    }
    chat_text_wire_chunk(
        id.unwrap_or("chatcmpl_prop"),
        model.unwrap_or("model-prop"),
        &text,
    )
}

proptest! {
    #[test]
    fn chat_completions_text_delta_wire_item_wire_roundtrips(
        id in "[a-zA-Z0-9_-]{1,24}",
        model in "[a-zA-Z0-9_.:-]{1,24}",
        text in "[a-zA-Z0-9 _.,:;!?/-]{1,64}",
    ) {
        let mut state = ChatCompletionsStreamState::default();
        let events = parse_stream_chunk_with_state(chat_text_wire_chunk(&id, &model, &text), &mut state)
            .expect("parse chat completions chunk");
        let projected = chat_text_wire_chunk_from_events(&events);
        let mut second_state = ChatCompletionsStreamState::default();
        let reparsed = parse_stream_chunk_with_state(projected, &mut second_state)
            .expect("reparse projected chat completions chunk");

        prop_assert_eq!(reparsed, events);
    }
}
