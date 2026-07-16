use std::collections::BTreeMap;
use std::collections::BTreeSet;

use codex_api::agent_protocol::AgentMessage;
use codex_api::agent_protocol::AgentRequest;
use codex_api::agent_protocol::ContentBlock;
use codex_api::agent_protocol::ImageSource;
use codex_api::agent_protocol::MessageRole;
use codex_api::agent_protocol::PROVIDER_FLAVOR_METADATA_KEY;
use codex_api::agent_protocol::ReasoningConfig;
use codex_api::agent_protocol::RequestMetadata;
use codex_api::agent_protocol::ToolChoice;
use codex_api::agent_protocol::ToolResultContent;
use codex_protocol::config_types::ReasoningSummary as ReasoningSummaryConfig;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ReasoningItemContent;
use codex_protocol::models::ReasoningItemReasoningSummary;
use codex_protocol::models::ReasoningProviderMetadata;
use codex_protocol::models::TranscriptItem;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ReasoningEffort as ReasoningEffortConfig;
use codex_tools::LoadableToolSpec;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::TOOL_SEARCH_TOOL_NAME;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde_json::Value;
use serde_json::json;

use crate::client_common::Prompt;

const MAX_PROVIDER_NEUTRAL_SEARCH_LOADED_FUNCTIONS: usize = 64;
const ANTHROPIC_REASONING_SIGNATURE_PREFIX: &str = "anthropic_signature:";

pub(crate) struct AgentRequestBuildParams<'a> {
    pub(crate) prompt: &'a Prompt,
    pub(crate) model_info: &'a ModelInfo,
    pub(crate) effort: Option<ReasoningEffortConfig>,
    pub(crate) summary: ReasoningSummaryConfig,
    pub(crate) service_tier: Option<String>,
    pub(crate) prompt_cache_key: String,
    pub(crate) provider_flavor: Option<String>,
    pub(crate) provider_request_body: Option<BTreeMap<String, Value>>,
    pub(crate) provider_request_body_remove: Vec<String>,
}

pub(crate) fn build_agent_request(params: AgentRequestBuildParams<'_>) -> Result<AgentRequest> {
    let formatted_input = params.prompt.get_formatted_input();
    let tool_specs = provider_neutral_tool_specs(&params.prompt.tools, &formatted_input);
    let freeform_tool_names = tool_specs
        .iter()
        .filter_map(|spec| match spec {
            ToolSpec::Freeform(tool) => Some(tool.name.clone()),
            ToolSpec::Function(_)
            | ToolSpec::Namespace(_)
            | ToolSpec::ToolSearch { .. }
            | ToolSpec::ImageGeneration { .. }
            | ToolSpec::WebSearch { .. } => None,
        })
        .collect();
    let tools = codex_tools::create_agent_tools_for_provider_neutral_request(&tool_specs)
        .map_err(|err| CodexErr::InvalidRequest(format!("failed to convert tools: {err}")))?;
    let messages = response_items_to_agent_messages(&formatted_input, &freeform_tool_names);

    let mut provider = params.provider_request_body.unwrap_or_default();
    if let Some(provider_flavor) = params.provider_flavor {
        provider.insert(
            PROVIDER_FLAVOR_METADATA_KEY.to_string(),
            Value::String(provider_flavor),
        );
    }
    for key in params.provider_request_body_remove {
        provider.insert(key, Value::Null);
    }

    Ok(AgentRequest {
        model: params.model_info.slug.clone(),
        instructions: vec![ContentBlock::Text {
            text: params.prompt.base_instructions.text.clone(),
        }],
        messages,
        tools,
        tool_choice: ToolChoice::Auto,
        parallel_tool_calls: params.prompt.parallel_tool_calls,
        stream: true,
        reasoning: build_reasoning_config(params.model_info, params.effort, params.summary),
        metadata: RequestMetadata {
            service_tier: params
                .model_info
                .service_tier_for_request(params.service_tier),
            prompt_cache_key: Some(params.prompt_cache_key),
            response_format: response_format_for_output_schema(
                &params.prompt.output_schema,
                params.prompt.output_schema_strict,
            ),
            provider,
        },
    })
}

fn response_format_for_output_schema(
    output_schema: &Option<Value>,
    output_schema_strict: bool,
) -> Option<Value> {
    output_schema.as_ref().map(|schema| {
        json!({
            "type": "json_schema",
            "json_schema": {
                "name": "codex_output_schema",
                "strict": output_schema_strict,
                "schema": schema,
            },
        })
    })
}

fn provider_neutral_tool_specs(base_tools: &[ToolSpec], input: &[TranscriptItem]) -> Vec<ToolSpec> {
    let mut tools = base_tools.to_vec();
    let mut seen_names = provider_neutral_tool_names_for_specs(base_tools);

    for loadable_tool in collect_search_loaded_tool_specs(input) {
        let spec = ToolSpec::from(loadable_tool);
        let names = provider_neutral_tool_names_for_spec(&spec);
        if names.is_empty() || names.iter().any(|name| seen_names.contains(name)) {
            continue;
        }

        seen_names.extend(names);
        tools.push(spec);
    }

    tools
}

fn provider_neutral_tool_names_for_specs(tools: &[ToolSpec]) -> BTreeSet<String> {
    tools
        .iter()
        .flat_map(provider_neutral_tool_names_for_spec)
        .collect()
}

fn provider_neutral_tool_names_for_spec(spec: &ToolSpec) -> Vec<String> {
    match spec {
        ToolSpec::Function(tool) => vec![tool.name.clone()],
        ToolSpec::Namespace(namespace) => namespace
            .tools
            .iter()
            .map(|tool| match tool {
                ResponsesApiNamespaceTool::Function(tool) => {
                    codex_tools::provider_neutral_tool_name_for_tool_name(&ToolName::namespaced(
                        namespace.name.clone(),
                        tool.name.clone(),
                    ))
                }
            })
            .collect(),
        ToolSpec::ToolSearch { .. } => vec![spec.name().to_string()],
        ToolSpec::ImageGeneration { .. } => vec![spec.name().to_string()],
        ToolSpec::WebSearch { .. } => vec![spec.name().to_string()],
        ToolSpec::Freeform(tool) => vec![tool.name.clone()],
    }
}

fn collect_search_loaded_tool_specs(input: &[TranscriptItem]) -> Vec<LoadableToolSpec> {
    let mut loaded_tools = Vec::new();
    let mut function_count = 0usize;

    for item in input.iter().rev() {
        let TranscriptItem::ToolSearchOutput {
            execution, tools, ..
        } = item
        else {
            continue;
        };
        if execution != "client" {
            continue;
        }

        for tool in tools.iter().rev() {
            let Ok(tool) = serde_json::from_value::<LoadableToolSpec>(tool.clone()) else {
                continue;
            };
            let tool_function_count = loadable_tool_function_count(&tool);
            if tool_function_count == 0 {
                continue;
            }
            if function_count + tool_function_count > MAX_PROVIDER_NEUTRAL_SEARCH_LOADED_FUNCTIONS {
                continue;
            }

            loaded_tools.push(tool);
            function_count += tool_function_count;
            if function_count == MAX_PROVIDER_NEUTRAL_SEARCH_LOADED_FUNCTIONS {
                break;
            }
        }
        if function_count == MAX_PROVIDER_NEUTRAL_SEARCH_LOADED_FUNCTIONS {
            break;
        }
    }

    loaded_tools.reverse();
    loaded_tools
}

fn loadable_tool_function_count(tool: &LoadableToolSpec) -> usize {
    match tool {
        LoadableToolSpec::Function(_) => 1,
        LoadableToolSpec::Namespace(namespace) => namespace.tools.len(),
    }
}

fn build_reasoning_config(
    model_info: &ModelInfo,
    effort: Option<ReasoningEffortConfig>,
    summary: ReasoningSummaryConfig,
) -> Option<ReasoningConfig> {
    let can_send_effort = effort.is_some()
        || model_info.default_reasoning_level.is_some()
        || !model_info.supported_reasoning_levels.is_empty();
    let effort = can_send_effort
        .then(|| effort.or_else(|| model_info.default_reasoning_level.clone()))
        .flatten()
        .map(|effort| effort.to_string());
    let summary = (model_info.supports_reasoning_summaries
        && summary != ReasoningSummaryConfig::None)
        .then(|| summary.to_string());

    if effort.is_none() && summary.is_none() {
        return None;
    }

    Some(ReasoningConfig { effort, summary })
}

fn response_items_to_agent_messages(
    items: &[TranscriptItem],
    freeform_tool_names: &BTreeSet<String>,
) -> Vec<AgentMessage> {
    let mut skipped_function_call_ids = BTreeSet::new();
    items
        .iter()
        .filter_map(|item| {
            response_item_to_agent_message(
                item,
                freeform_tool_names,
                &mut skipped_function_call_ids,
            )
        })
        .collect()
}

fn response_item_to_agent_message(
    item: &TranscriptItem,
    freeform_tool_names: &BTreeSet<String>,
    skipped_function_call_ids: &mut BTreeSet<String>,
) -> Option<AgentMessage> {
    match item {
        TranscriptItem::Message {
            id, role, content, ..
        } => Some(AgentMessage {
            role: message_role(role),
            content: content.iter().map(content_item_to_block).collect(),
            id: id.clone(),
        }),
        TranscriptItem::Reasoning {
            summary,
            content,
            encrypted_content,
            provider_metadata,
            ..
        } => reasoning_blocks(
            summary,
            content.as_deref(),
            encrypted_content.as_deref(),
            provider_metadata.as_ref(),
        )
        .map(|content| AgentMessage {
            role: MessageRole::Assistant,
            content,
            id: None,
        }),
        TranscriptItem::FunctionCall {
            name,
            arguments,
            call_id,
            ..
        } => {
            let Some(input) = parse_function_call_input(arguments) else {
                tracing::warn!(
                    call_id = %call_id,
                    tool_name = %name,
                    "dropping malformed function call from provider-neutral request history"
                );
                skipped_function_call_ids.insert(call_id.clone());
                return None;
            };

            Some(AgentMessage {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: call_id.clone(),
                    name: name.clone(),
                    input,
                }],
                id: None,
            })
        }
        TranscriptItem::CustomToolCall {
            name,
            input: arguments,
            call_id,
            ..
        } => Some(AgentMessage {
            role: MessageRole::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: call_id.clone(),
                name: name.clone(),
                input: if freeform_tool_names.contains(name) {
                    json!({ "input": arguments })
                } else {
                    parse_tool_input(arguments)
                },
            }],
            id: None,
        }),
        TranscriptItem::ToolSearchCall {
            call_id: Some(call_id),
            arguments,
            ..
        } => Some(AgentMessage {
            role: MessageRole::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: call_id.clone(),
                name: TOOL_SEARCH_TOOL_NAME.to_string(),
                input: arguments.clone(),
            }],
            id: None,
        }),
        TranscriptItem::FunctionCallOutput { call_id, output } => {
            if skipped_function_call_ids.contains(call_id) {
                return None;
            }

            Some(AgentMessage {
                role: MessageRole::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: call_id.clone(),
                    content: tool_result_content(output),
                    is_error: output.success == Some(false),
                }],
                id: None,
            })
        }
        TranscriptItem::CustomToolCallOutput {
            call_id, output, ..
        } => Some(AgentMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: call_id.clone(),
                content: tool_result_content(output),
                is_error: output.success == Some(false),
            }],
            id: None,
        }),
        TranscriptItem::ToolSearchOutput {
            call_id,
            status,
            execution,
            tools,
        } => call_id.as_ref().map(|tool_use_id| AgentMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.clone(),
                content: vec![ToolResultContent::Json {
                    value: serde_json::json!({
                        "status": status,
                        "execution": execution,
                        "tools": tools,
                    }),
                }],
                is_error: false,
            }],
            id: None,
        }),
        TranscriptItem::AgentMessage { .. }
        | TranscriptItem::LocalShellCall { .. }
        | TranscriptItem::ToolSearchCall { call_id: None, .. }
        | TranscriptItem::WebSearchCall { .. }
        | TranscriptItem::ImageGenerationCall { .. } => None,
        TranscriptItem::Compaction { encrypted_content } => Some(AgentMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::Compaction {
                text: encrypted_content.clone(),
            }],
            id: None,
        }),
        TranscriptItem::ContextCompaction {
            encrypted_content: Some(encrypted_content),
        } => Some(AgentMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::Compaction {
                text: encrypted_content.clone(),
            }],
            id: None,
        }),
        TranscriptItem::CompactionTrigger
        | TranscriptItem::ContextCompaction { .. }
        | TranscriptItem::Other => None,
    }
}

fn message_role(role: &str) -> MessageRole {
    match role {
        "assistant" => MessageRole::Assistant,
        "developer" => MessageRole::Developer,
        "system" => MessageRole::System,
        "user" => MessageRole::User,
        _ => MessageRole::User,
    }
}

fn content_item_to_block(item: &ContentItem) -> ContentBlock {
    match item {
        ContentItem::InputText { text } | ContentItem::OutputText { text } => {
            ContentBlock::Text { text: text.clone() }
        }
        ContentItem::InputImage { image_url, detail } => ContentBlock::Image {
            source: image_source(image_url),
            detail: detail.as_ref().and_then(image_detail_string),
        },
    }
}

fn image_detail_string<T>(detail: &T) -> Option<String>
where
    T: serde::Serialize,
{
    serde_json::to_value(detail)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
}

fn image_source(image_url: &str) -> ImageSource {
    if let Some(data_url) = image_url.strip_prefix("data:")
        && let Some((media_type, data)) = data_url.split_once(";base64,")
    {
        return ImageSource::Base64 {
            media_type: media_type.to_string(),
            data: data.to_string(),
        };
    }

    ImageSource::Url {
        url: image_url.to_string(),
    }
}

fn reasoning_blocks(
    summary: &[ReasoningItemReasoningSummary],
    content: Option<&[ReasoningItemContent]>,
    encrypted_content: Option<&str>,
    provider_metadata: Option<&ReasoningProviderMetadata>,
) -> Option<Vec<ContentBlock>> {
    let reasoning_text = content
        .into_iter()
        .flatten()
        .map(|item| match item {
            ReasoningItemContent::ReasoningText { text } | ReasoningItemContent::Text { text } => {
                text.as_str()
            }
        })
        .chain(summary.iter().map(|item| match item {
            ReasoningItemReasoningSummary::SummaryText { text } => text.as_str(),
        }))
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    (!reasoning_text.is_empty()).then(|| {
        let signature = provider_metadata
            .and_then(|metadata| metadata.anthropic_signature.clone())
            .or_else(|| {
                encrypted_content.and_then(|content| {
                    content
                        .strip_prefix(ANTHROPIC_REASONING_SIGNATURE_PREFIX)
                        .map(str::to_string)
                })
            });
        vec![ContentBlock::Reasoning {
            text: reasoning_text,
            signature,
        }]
    })
}

fn parse_tool_input(input: &str) -> Value {
    serde_json::from_str(input).unwrap_or_else(|_| Value::String(input.to_string()))
}

fn parse_function_call_input(input: &str) -> Option<Value> {
    match serde_json::from_str(input) {
        Ok(value @ Value::Object(_)) => Some(value),
        _ => None,
    }
}

fn tool_result_content(output: &FunctionCallOutputPayload) -> Vec<ToolResultContent> {
    match &output.body {
        FunctionCallOutputBody::Text(text) => vec![ToolResultContent::Text { text: text.clone() }],
        FunctionCallOutputBody::ContentItems(items) => {
            items.iter().filter_map(tool_result_content_item).collect()
        }
    }
}

fn tool_result_content_item(item: &FunctionCallOutputContentItem) -> Option<ToolResultContent> {
    match item {
        FunctionCallOutputContentItem::InputText { text } => {
            Some(ToolResultContent::Text { text: text.clone() })
        }
        FunctionCallOutputContentItem::InputImage { image_url, detail } => {
            Some(ToolResultContent::Image {
                source: image_source(image_url),
                detail: detail.as_ref().and_then(image_detail_string),
            })
        }
        FunctionCallOutputContentItem::EncryptedContent { .. } => None,
    }
}

#[cfg(test)]
#[path = "agent_request_tests.rs"]
mod tests;
