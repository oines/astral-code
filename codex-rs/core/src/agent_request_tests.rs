use super::*;
use codex_api::agent_protocol::AgentTool;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ReasoningItemReasoningSummary;
use codex_protocol::openai_models::ReasoningEffort;
use codex_tools::AdditionalProperties;
use codex_tools::JsonSchema;
use codex_tools::LoadableToolSpec;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::collections::BTreeMap;

fn test_model_info(supports_reasoning_summaries: bool) -> ModelInfo {
    serde_json::from_value(json!({
        "slug": "deepseek-v4-pro",
        "display_name": "deepseek-v4-pro",
        "description": "desc",
        "default_reasoning_level": "medium",
        "supported_reasoning_levels": [
            {"effort": "medium", "description": "medium"},
            {"effort": "high", "description": "high"}
        ],
        "shell_type": "shell_command",
        "visibility": "list",
        "supported_in_api": true,
        "priority": 1,
        "upgrade": null,
        "base_instructions": "base instructions",
        "model_messages": null,
        "supports_reasoning_summaries": supports_reasoning_summaries,
        "support_verbosity": false,
        "default_verbosity": null,
        "apply_patch_tool_type": null,
        "truncation_policy": {"mode": "bytes", "limit": 10000},
        "supports_parallel_tool_calls": true,
        "supports_image_detail_original": false,
        "context_window": 272000,
        "auto_compact_token_limit": null,
        "service_tiers": [{
            "id": "priority",
            "name": "Priority",
            "description": "Priority routing"
        }],
        "experimental_supported_tools": []
    }))
    .expect("deserialize test model info")
}

fn bash_tool_spec() -> ToolSpec {
    ToolSpec::Function(ResponsesApiTool {
        name: "Bash".to_string(),
        description: "Run a shell command".to_string(),
        strict: true,
        defer_loading: None,
        parameters: JsonSchema::object(
            BTreeMap::from([(
                "command".to_string(),
                JsonSchema::string(/*description*/ None),
            )]),
            Some(vec!["command".to_string()]),
            Some(AdditionalProperties::Boolean(false)),
        ),
        output_schema: None,
    })
}

fn test_function_tool(name: &str, description: &str) -> ResponsesApiTool {
    ResponsesApiTool {
        name: name.to_string(),
        description: description.to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            BTreeMap::new(),
            /*required*/ None,
            /*additional_properties*/ None,
        ),
        output_schema: None,
    }
}

#[test]
fn build_agent_request_maps_prompt_history_tools_and_metadata() {
    let prompt = Prompt {
        input: vec![
            ResponseItem::Message {
                id: Some("msg-user".to_string()),
                role: "user".to_string(),
                content: vec![
                    ContentItem::InputText {
                        text: "inspect the repo".to_string(),
                    },
                    ContentItem::InputImage {
                        image_url: "data:image/png;base64,abc123".to_string(),
                        detail: None,
                    },
                ],
                phase: None,
            },
            ResponseItem::Reasoning {
                id: "rs-1".to_string(),
                summary: vec![ReasoningItemReasoningSummary::SummaryText {
                    text: "checked the tool plan".to_string(),
                }],
                content: Some(vec![ReasoningItemContent::ReasoningText {
                    text: "need a shell call".to_string(),
                }]),
                encrypted_content: None,
            },
            ResponseItem::FunctionCall {
                id: Some("fc-1".to_string()),
                name: "Bash".to_string(),
                namespace: None,
                arguments: r#"{"command":"pwd"}"#.to_string(),
                call_id: "call-1".to_string(),
            },
            ResponseItem::FunctionCallOutput {
                call_id: "call-1".to_string(),
                output: FunctionCallOutputPayload {
                    body: FunctionCallOutputBody::Text("/tmp/project".to_string()),
                    success: Some(true),
                },
            },
        ],
        tools: vec![bash_tool_spec()],
        parallel_tool_calls: true,
        base_instructions: codex_protocol::models::BaseInstructions {
            text: "You are Astral.".to_string(),
        },
        personality: None,
        output_schema: None,
        output_schema_strict: true,
    };

    let request = build_agent_request(AgentRequestBuildParams {
        prompt: &prompt,
        model_info: &test_model_info(/*supports_reasoning_summaries*/ true),
        effort: Some(ReasoningEffort::High),
        summary: ReasoningSummaryConfig::Concise,
        service_tier: Some("priority".to_string()),
        prompt_cache_key: "thread-1".to_string(),
        provider_request_body: Some(BTreeMap::from([("temperature".to_string(), json!(0.2))])),
        provider_request_body_remove: vec!["stream_options".to_string()],
        provider_flavor: None,
    })
    .expect("build agent request");

    assert_eq!(
        request,
        AgentRequest {
            model: "deepseek-v4-pro".to_string(),
            instructions: vec![ContentBlock::Text {
                text: "You are Astral.".to_string(),
            }],
            messages: vec![
                AgentMessage {
                    role: MessageRole::User,
                    content: vec![
                        ContentBlock::Text {
                            text: "inspect the repo".to_string(),
                        },
                        ContentBlock::Image {
                            source: ImageSource::Base64 {
                                media_type: "image/png".to_string(),
                                data: "abc123".to_string(),
                            },
                        },
                    ],
                    id: Some("msg-user".to_string()),
                },
                AgentMessage {
                    role: MessageRole::Assistant,
                    content: vec![ContentBlock::Reasoning {
                        text: "need a shell call\nchecked the tool plan".to_string(),
                        signature: None,
                    }],
                    id: None,
                },
                AgentMessage {
                    role: MessageRole::Assistant,
                    content: vec![ContentBlock::ToolUse {
                        id: "call-1".to_string(),
                        name: "Bash".to_string(),
                        input: json!({ "command": "pwd" }),
                    }],
                    id: None,
                },
                AgentMessage {
                    role: MessageRole::User,
                    content: vec![ContentBlock::ToolResult {
                        tool_use_id: "call-1".to_string(),
                        content: vec![ToolResultContent::Text {
                            text: "/tmp/project".to_string(),
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
                    "properties": {
                        "command": { "type": "string" }
                    },
                    "required": ["command"],
                    "additionalProperties": false,
                }),
                metadata: BTreeMap::from([("strict".to_string(), json!(true))]),
            }],
            tool_choice: ToolChoice::Auto,
            parallel_tool_calls: true,
            stream: true,
            reasoning: Some(ReasoningConfig {
                effort: Some("high".to_string()),
                summary: Some("concise".to_string()),
            }),
            metadata: RequestMetadata {
                service_tier: Some("priority".to_string()),
                prompt_cache_key: Some("thread-1".to_string()),
                response_format: None,
                provider: BTreeMap::from([
                    ("stream_options".to_string(), serde_json::Value::Null),
                    ("temperature".to_string(), json!(0.2)),
                ]),
            },
        }
    );
}

#[test]
fn build_agent_request_loads_recent_tool_search_results() {
    let prompt = Prompt {
        input: vec![ResponseItem::ToolSearchOutput {
            call_id: None,
            status: "completed".to_string(),
            execution: "client".to_string(),
            tools: vec![
                serde_json::to_value(LoadableToolSpec::Function(test_function_tool(
                    "plain_loaded",
                    "Loaded duplicate should not override direct tool.",
                )))
                .expect("serialize loadable tool"),
                serde_json::to_value(LoadableToolSpec::Namespace(ResponsesApiNamespace {
                    name: "multi_agent_v1".to_string(),
                    description: "Tools for spawning and managing sub-agents.".to_string(),
                    tools: vec![ResponsesApiNamespaceTool::Function(test_function_tool(
                        "spawn_agent",
                        "Spawn a sub-agent.",
                    ))],
                }))
                .expect("serialize loadable namespace"),
            ],
        }],
        tools: vec![
            bash_tool_spec(),
            ToolSpec::Function(test_function_tool(
                "plain_loaded",
                "Direct tool wins over loaded duplicate.",
            )),
        ],
        ..Prompt::default()
    };

    let request = build_agent_request(AgentRequestBuildParams {
        prompt: &prompt,
        model_info: &test_model_info(/*supports_reasoning_summaries*/ false),
        effort: None,
        summary: ReasoningSummaryConfig::None,
        service_tier: None,
        prompt_cache_key: "thread-1".to_string(),
        provider_request_body: None,
        provider_request_body_remove: Vec::new(),
        provider_flavor: None,
    })
    .expect("build agent request");

    assert_eq!(
        request.tools,
        vec![
            AgentTool {
                name: "Bash".to_string(),
                description: "Run a shell command".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string" }
                    },
                    "required": ["command"],
                    "additionalProperties": false,
                }),
                metadata: BTreeMap::from([("strict".to_string(), json!(true))]),
            },
            AgentTool {
                name: "plain_loaded".to_string(),
                description: "Direct tool wins over loaded duplicate.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                }),
                metadata: BTreeMap::new(),
            },
            AgentTool {
                name: "multi_agent_v1__spawn_agent".to_string(),
                description: "Spawn a sub-agent.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                }),
                metadata: BTreeMap::from([
                    ("namespace".to_string(), json!("multi_agent_v1")),
                    (
                        "namespaceDescription".to_string(),
                        json!("Tools for spawning and managing sub-agents.")
                    ),
                    ("originalName".to_string(), json!("spawn_agent")),
                ]),
            },
        ]
    );
}

#[test]
fn build_agent_request_filters_malformed_function_calls_without_dropping_valid_history() {
    let prompt = Prompt {
        input: vec![
            ResponseItem::Message {
                id: Some("before".to_string()),
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "before bad call".to_string(),
                }],
                phase: None,
            },
            ResponseItem::FunctionCall {
                id: Some("bad-fc".to_string()),
                name: "Bash".to_string(),
                namespace: None,
                arguments: r#"{"limit": "#.to_string(),
                call_id: "call-bad".to_string(),
            },
            ResponseItem::FunctionCallOutput {
                call_id: "call-bad".to_string(),
                output: FunctionCallOutputPayload::from_text("bad output".to_string()),
            },
            ResponseItem::Message {
                id: Some("after".to_string()),
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText {
                    text: "after bad call".to_string(),
                }],
                phase: None,
            },
            ResponseItem::FunctionCall {
                id: Some("good-fc".to_string()),
                name: "Bash".to_string(),
                namespace: None,
                arguments: r#"{"command":"pwd"}"#.to_string(),
                call_id: "call-good".to_string(),
            },
            ResponseItem::FunctionCallOutput {
                call_id: "call-good".to_string(),
                output: FunctionCallOutputPayload::from_text("/tmp/project".to_string()),
            },
            ResponseItem::CustomToolCall {
                id: Some("custom-fc".to_string()),
                status: Some("completed".to_string()),
                call_id: "call-custom".to_string(),
                name: "apply_patch".to_string(),
                input: "*** Begin Patch\nraw patch text\n*** End Patch".to_string(),
            },
            ResponseItem::CustomToolCallOutput {
                call_id: "call-custom".to_string(),
                name: Some("apply_patch".to_string()),
                output: FunctionCallOutputPayload::from_text("patch applied".to_string()),
            },
        ],
        tools: vec![bash_tool_spec()],
        ..Prompt::default()
    };

    let request = build_agent_request(AgentRequestBuildParams {
        prompt: &prompt,
        model_info: &test_model_info(/*supports_reasoning_summaries*/ false),
        effort: None,
        summary: ReasoningSummaryConfig::None,
        service_tier: None,
        prompt_cache_key: "thread-1".to_string(),
        provider_request_body: None,
        provider_request_body_remove: Vec::new(),
        provider_flavor: None,
    })
    .expect("build agent request");

    assert_eq!(
        request.messages,
        vec![
            AgentMessage {
                role: MessageRole::User,
                content: vec![ContentBlock::Text {
                    text: "before bad call".to_string(),
                }],
                id: Some("before".to_string()),
            },
            AgentMessage {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Text {
                    text: "after bad call".to_string(),
                }],
                id: Some("after".to_string()),
            },
            AgentMessage {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "call-good".to_string(),
                    name: "Bash".to_string(),
                    input: json!({ "command": "pwd" }),
                }],
                id: None,
            },
            AgentMessage {
                role: MessageRole::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call-good".to_string(),
                    content: vec![ToolResultContent::Text {
                        text: "/tmp/project".to_string(),
                    }],
                    is_error: false,
                }],
                id: None,
            },
            AgentMessage {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "call-custom".to_string(),
                    name: "apply_patch".to_string(),
                    input: json!("*** Begin Patch\nraw patch text\n*** End Patch"),
                }],
                id: None,
            },
            AgentMessage {
                role: MessageRole::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call-custom".to_string(),
                    content: vec![ToolResultContent::Text {
                        text: "patch applied".to_string(),
                    }],
                    is_error: false,
                }],
                id: None,
            },
        ]
    );
}

#[test]
fn build_agent_request_rejects_responses_only_hosted_tools() {
    let prompt = Prompt {
        tools: vec![ToolSpec::WebSearch {
            external_web_access: None,
            filters: None,
            user_location: None,
            search_context_size: None,
            search_content_types: None,
        }],
        ..Prompt::default()
    };

    let err = build_agent_request(AgentRequestBuildParams {
        prompt: &prompt,
        model_info: &test_model_info(/*supports_reasoning_summaries*/ false),
        effort: None,
        summary: ReasoningSummaryConfig::None,
        service_tier: None,
        prompt_cache_key: "thread-1".to_string(),
        provider_request_body: None,
        provider_request_body_remove: Vec::new(),
        provider_flavor: None,
    })
    .expect_err("hosted tools should not convert to provider-neutral tools");

    match err {
        CodexErr::InvalidRequest(message) => assert!(
            message.contains("web_search"),
            "unexpected error message: {message}"
        ),
        other => panic!("expected invalid request, got {other:?}"),
    }
}
