use crate::client_common::Prompt;
use crate::context_manager::strip_images_when_unsupported;
use codex_api::Reasoning;
use codex_api::ResponsesApiRequest;
use codex_api::ResponsesTextControls;
use codex_api::ResponsesTextFormat;
use codex_protocol::config_types::ReasoningSummary as ReasoningSummaryConfig;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::TranscriptItem;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ReasoningEffort as ReasoningEffortConfig;
use codex_tools::create_tools_json_for_responses_api;

pub(crate) struct ResponsesRequestParams<'a> {
    pub(crate) prompt: &'a Prompt,
    pub(crate) model_info: &'a ModelInfo,
    pub(crate) effort: Option<ReasoningEffortConfig>,
    pub(crate) summary: ReasoningSummaryConfig,
    pub(crate) service_tier: Option<String>,
    pub(crate) prompt_cache_key: String,
}

pub(crate) fn build_responses_request(
    params: ResponsesRequestParams<'_>,
) -> codex_protocol::error::Result<ResponsesApiRequest> {
    let ResponsesRequestParams {
        prompt,
        model_info,
        effort,
        summary,
        service_tier,
        prompt_cache_key,
    } = params;
    let mut input = project_responses_input(prompt.get_formatted_input());
    strip_images_when_unsupported(&model_info.input_modalities, &mut input);
    let tools = create_tools_json_for_responses_api(&prompt.tools)
        .map_err(|err| codex_protocol::error::CodexErr::InvalidRequest(err.to_string()))?;
    let reasoning = build_reasoning(model_info, effort, summary);
    let include = if reasoning.is_some() {
        vec!["reasoning.encrypted_content".to_string()]
    } else {
        Default::default()
    };
    let text = prompt
        .output_schema
        .clone()
        .map(|schema| ResponsesTextControls {
            format: ResponsesTextFormat {
                r#type: "json_schema".to_string(),
                name: "astral_output_schema".to_string(),
                strict: prompt.output_schema_strict,
                schema,
            },
        });

    Ok(ResponsesApiRequest {
        model: model_info.slug.clone(),
        instructions: prompt.base_instructions.text.clone(),
        input,
        tools,
        tool_choice: "auto".to_string(),
        parallel_tool_calls: prompt.parallel_tool_calls,
        reasoning,
        store: false,
        stream: true,
        include,
        service_tier: model_info.service_tier_for_request(service_tier),
        prompt_cache_key: Some(prompt_cache_key),
        text,
    })
}

fn project_responses_input(input: Vec<TranscriptItem>) -> Vec<TranscriptItem> {
    input
        .into_iter()
        .map(|item| match item {
            TranscriptItem::LocalCompaction { text } => TranscriptItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText { text }],
                phase: None,
            },
            item => item,
        })
        .collect()
}

pub(crate) fn strip_responses_encrypted_state(
    input: Vec<TranscriptItem>,
) -> (Vec<TranscriptItem>, usize) {
    let mut removed = 0usize;
    let input = input
        .into_iter()
        .filter_map(|item| match item {
            TranscriptItem::Reasoning {
                encrypted_content: Some(_),
                ..
            }
            | TranscriptItem::Compaction { .. }
            | TranscriptItem::ContextCompaction {
                encrypted_content: Some(_),
            } => {
                removed += 1;
                None
            }
            TranscriptItem::AgentMessage { content, .. }
                if content.iter().any(|content| {
                    matches!(
                        content,
                        codex_protocol::models::AgentMessageInputContent::EncryptedContent { .. }
                    )
                }) =>
            {
                removed += 1;
                None
            }
            TranscriptItem::FunctionCallOutput {
                call_id,
                mut output,
            } => {
                removed += strip_encrypted_tool_output(&mut output.body);
                Some(TranscriptItem::FunctionCallOutput { call_id, output })
            }
            TranscriptItem::CustomToolCallOutput {
                call_id,
                name,
                mut output,
            } => {
                removed += strip_encrypted_tool_output(&mut output.body);
                Some(TranscriptItem::CustomToolCallOutput {
                    call_id,
                    name,
                    output,
                })
            }
            item => Some(item),
        })
        .collect();
    (input, removed)
}

fn strip_encrypted_tool_output(output: &mut FunctionCallOutputBody) -> usize {
    let FunctionCallOutputBody::ContentItems(items) = output else {
        return 0;
    };
    let original_len = items.len();
    items.retain(|item| !matches!(item, FunctionCallOutputContentItem::EncryptedContent { .. }));
    let removed = original_len.saturating_sub(items.len());
    if removed > 0 && items.is_empty() {
        *output = FunctionCallOutputBody::Text(
            "[encrypted tool output unavailable after provider state reset]".to_string(),
        );
    }
    removed
}

fn build_reasoning(
    model_info: &ModelInfo,
    effort: Option<ReasoningEffortConfig>,
    summary: ReasoningSummaryConfig,
) -> Option<Reasoning> {
    let can_send_effort = effort.is_some()
        || model_info.default_reasoning_level.is_some()
        || !model_info.supported_reasoning_levels.is_empty();
    let effort = can_send_effort
        .then(|| effort.or_else(|| model_info.default_reasoning_level.clone()))
        .flatten();
    let summary = (model_info.supports_reasoning_summaries
        && summary != ReasoningSummaryConfig::None)
        .then(|| summary.to_string());
    if effort.is_none() && summary.is_none() {
        None
    } else {
        Some(Reasoning {
            effort,
            summary,
            context: None,
        })
    }
}

#[cfg(test)]
#[path = "responses_request_tests.rs"]
mod tests;
