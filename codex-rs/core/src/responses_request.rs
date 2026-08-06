use crate::client_common::Prompt;
use crate::context_manager::strip_images_when_unsupported;
use codex_api::Reasoning;
use codex_api::ResponsesApiRequest;
use codex_api::ResponsesTextControls;
use codex_api::ResponsesTextFormat;
use codex_model_provider_info::ResponsesBuiltinTools;
use codex_protocol::config_types::ReasoningSummary as ReasoningSummaryConfig;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ReasoningEffort as ReasoningEffortConfig;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ToolSpec;
use codex_tools::create_tools_json_for_responses_api;
use std::collections::BTreeSet;

pub(crate) struct ResponsesRequestParams<'a> {
    pub(crate) prompt: &'a Prompt,
    pub(crate) model_info: &'a ModelInfo,
    pub(crate) effort: Option<ReasoningEffortConfig>,
    pub(crate) summary: ReasoningSummaryConfig,
    pub(crate) service_tier: Option<String>,
    pub(crate) prompt_cache_key: String,
    pub(crate) builtin_tools: &'a ResponsesBuiltinTools,
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
        builtin_tools,
    } = params;
    let mut input = prompt.get_formatted_input();
    strip_images_when_unsupported(&model_info.input_modalities, &mut input);
    let tools = select_tools(&prompt.tools, builtin_tools);
    let tools = create_tools_json_for_responses_api(&tools)
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
        Some(Reasoning { effort, summary })
    }
}

fn select_tools(tools: &[ToolSpec], builtin_tools: &ResponsesBuiltinTools) -> Vec<ToolSpec> {
    let local_names = tools
        .iter()
        .filter(|tool| !is_provider_hosted(tool))
        .map(ToolSpec::name)
        .collect::<BTreeSet<_>>();
    let active_builtin_names = tools
        .iter()
        .filter(|tool| is_provider_hosted(tool) && builtin_tools.allows(tool.name()))
        .map(ToolSpec::name)
        .collect::<BTreeSet<_>>();
    let explicit_selection = builtin_tools.is_explicit_selection();

    tools
        .iter()
        .filter_map(|tool| {
            if is_provider_hosted(tool) {
                (active_builtin_names.contains(tool.name())
                    && (explicit_selection || !local_names.contains(tool.name())))
                .then(|| tool.clone())
            } else if explicit_selection {
                remove_explicit_builtin_collisions(tool, &active_builtin_names)
            } else {
                Some(tool.clone())
            }
        })
        .collect()
}

fn remove_explicit_builtin_collisions(
    tool: &ToolSpec,
    active_builtin_names: &BTreeSet<&str>,
) -> Option<ToolSpec> {
    if active_builtin_names.contains(tool.name()) {
        return None;
    }

    let ToolSpec::Namespace(namespace) = tool else {
        return Some(tool.clone());
    };
    let mut namespace = namespace.clone();
    let namespace_name = namespace.name.clone();
    namespace.tools.retain(|tool| match tool {
        ResponsesApiNamespaceTool::Function(tool) => {
            let hosted_name = format!("{namespace_name}_{}", tool.name);
            !active_builtin_names.contains(hosted_name.as_str())
        }
    });
    (!namespace.tools.is_empty()).then_some(ToolSpec::Namespace(namespace))
}

fn is_provider_hosted(tool: &ToolSpec) -> bool {
    matches!(
        tool,
        ToolSpec::ToolSearch { execution, .. } if execution == "server"
    ) || matches!(
        tool,
        ToolSpec::ImageGeneration { .. } | ToolSpec::WebSearch { .. }
    )
}

#[cfg(test)]
#[path = "responses_request_tests.rs"]
mod tests;
