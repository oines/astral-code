use codex_api::agent_protocol::AgentMessage;
use codex_api::agent_protocol::AgentRequest;
use codex_api::agent_protocol::ContentBlock;
use codex_api::agent_protocol::ImageSource;
use codex_api::agent_protocol::MessageRole;
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
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ReasoningEffort as ReasoningEffortConfig;
use serde_json::Value;

use crate::client_common::Prompt;

pub(crate) struct AgentRequestBuildParams<'a> {
    pub(crate) prompt: &'a Prompt,
    pub(crate) model_info: &'a ModelInfo,
    pub(crate) effort: Option<ReasoningEffortConfig>,
    pub(crate) summary: ReasoningSummaryConfig,
    pub(crate) service_tier: Option<String>,
    pub(crate) prompt_cache_key: String,
}

pub(crate) fn build_agent_request(params: AgentRequestBuildParams<'_>) -> Result<AgentRequest> {
    let tools = codex_tools::create_agent_tools_for_provider_neutral_request(&params.prompt.tools)
        .map_err(|err| CodexErr::InvalidRequest(format!("failed to convert tools: {err}")))?;
    let messages = params
        .prompt
        .get_formatted_input()
        .iter()
        .filter_map(response_item_to_agent_message)
        .collect();

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
            service_tier: params.service_tier,
            prompt_cache_key: Some(params.prompt_cache_key),
            provider: Default::default(),
        },
    })
}

fn build_reasoning_config(
    model_info: &ModelInfo,
    effort: Option<ReasoningEffortConfig>,
    summary: ReasoningSummaryConfig,
) -> Option<ReasoningConfig> {
    if !model_info.supports_reasoning_summaries {
        return None;
    }

    let effort = effort
        .or_else(|| model_info.default_reasoning_level.clone())
        .map(|effort| effort.to_string());
    let summary = (summary != ReasoningSummaryConfig::None).then(|| summary.to_string());

    if effort.is_none() && summary.is_none() {
        return None;
    }

    Some(ReasoningConfig { effort, summary })
}

fn response_item_to_agent_message(item: &ResponseItem) -> Option<AgentMessage> {
    match item {
        ResponseItem::Message {
            id, role, content, ..
        } => Some(AgentMessage {
            role: message_role(role),
            content: content.iter().map(content_item_to_block).collect(),
            id: id.clone(),
        }),
        ResponseItem::Reasoning {
            summary, content, ..
        } => reasoning_blocks(summary, content.as_deref()).map(|content| AgentMessage {
            role: MessageRole::Assistant,
            content,
            id: None,
        }),
        ResponseItem::FunctionCall {
            name,
            arguments,
            call_id,
            ..
        }
        | ResponseItem::CustomToolCall {
            name,
            input: arguments,
            call_id,
            ..
        } => Some(AgentMessage {
            role: MessageRole::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: call_id.clone(),
                name: name.clone(),
                input: parse_tool_input(arguments),
            }],
            id: None,
        }),
        ResponseItem::FunctionCallOutput { call_id, output }
        | ResponseItem::CustomToolCallOutput {
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
        ResponseItem::ToolSearchOutput {
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
        ResponseItem::AgentMessage { .. }
        | ResponseItem::LocalShellCall { .. }
        | ResponseItem::ToolSearchCall { .. }
        | ResponseItem::WebSearchCall { .. }
        | ResponseItem::ImageGenerationCall { .. }
        | ResponseItem::Compaction { .. }
        | ResponseItem::CompactionTrigger
        | ResponseItem::ContextCompaction { .. }
        | ResponseItem::Other => None,
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
        ContentItem::InputImage { image_url, .. } => ContentBlock::Image {
            source: image_source(image_url),
        },
    }
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
        vec![ContentBlock::Reasoning {
            text: reasoning_text,
            signature: None,
        }]
    })
}

fn parse_tool_input(input: &str) -> Value {
    serde_json::from_str(input).unwrap_or_else(|_| Value::String(input.to_string()))
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
        FunctionCallOutputContentItem::InputImage { image_url, .. } => {
            Some(ToolResultContent::Image {
                source: image_source(image_url),
            })
        }
        FunctionCallOutputContentItem::EncryptedContent { .. } => None,
    }
}

#[cfg(test)]
#[path = "agent_request_tests.rs"]
mod tests;
