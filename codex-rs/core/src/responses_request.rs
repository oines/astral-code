use crate::client_common::Prompt;
use crate::context_manager::strip_images_when_unsupported;
use codex_api::Reasoning;
use codex_api::ReasoningContext;
use codex_api::ResponsesApiRequest;
use codex_api::ResponsesTextControls;
use codex_api::ResponsesTextFormat;
use codex_model_provider_info::ManagedAuthKind;
use codex_model_provider_info::ResponsesBuiltinTools;
use codex_protocol::config_types::ReasoningSummary as ReasoningSummaryConfig;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::TranscriptItem;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ReasoningEffort as ReasoningEffortConfig;
use codex_tools::AdditionalProperties;
use codex_tools::JsonSchema;
use codex_tools::JsonSchemaPrimitiveType;
use codex_tools::JsonSchemaType;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use codex_tools::create_tools_json_for_responses_api;
use codex_tools::provider_neutral_tool_name_for_tool_name;
use std::collections::BTreeSet;

pub(crate) struct ResponsesRequestParams<'a> {
    pub(crate) prompt: &'a Prompt,
    pub(crate) model_info: &'a ModelInfo,
    pub(crate) effort: Option<ReasoningEffortConfig>,
    pub(crate) summary: ReasoningSummaryConfig,
    pub(crate) service_tier: Option<String>,
    pub(crate) prompt_cache_key: String,
    pub(crate) builtin_tools: &'a ResponsesBuiltinTools,
    pub(crate) managed_auth: Option<ManagedAuthKind>,
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
        managed_auth,
    } = params;
    let mut input = project_responses_input(prompt.get_formatted_input());
    strip_images_when_unsupported(&model_info.input_modalities, &mut input);
    let mut tools = select_tools(&prompt.tools, builtin_tools);
    if managed_auth == Some(ManagedAuthKind::CodexOAuth) {
        flatten_reserved_codex_namespaces(&mut tools);
        relax_incompatible_strict_tools(&mut tools);
    }
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
        parallel_tool_calls: prompt.parallel_tool_calls && !model_info.use_responses_lite,
        reasoning,
        store: false,
        stream: true,
        include,
        service_tier: model_info.service_tier_for_request(service_tier),
        prompt_cache_key: Some(prompt_cache_key),
        text,
    })
}

fn flatten_reserved_codex_namespaces(tools: &mut Vec<ToolSpec>) {
    let mut projected = Vec::with_capacity(tools.len());
    for tool in tools.drain(..) {
        match tool {
            ToolSpec::Namespace(namespace) if namespace.name == "web" => {
                for tool in namespace.tools {
                    match tool {
                        ResponsesApiNamespaceTool::Function(mut tool) => {
                            tool.name = provider_neutral_tool_name_for_tool_name(
                                &codex_tools::ToolName::namespaced("web", tool.name),
                            );
                            projected.push(ToolSpec::Function(tool));
                        }
                    }
                }
            }
            tool => projected.push(tool),
        }
    }
    *tools = projected;
}

fn relax_incompatible_strict_tools(tools: &mut [ToolSpec]) {
    for tool in tools {
        match tool {
            ToolSpec::Function(tool) => relax_incompatible_strict_tool(tool),
            ToolSpec::Namespace(namespace) => {
                for tool in &mut namespace.tools {
                    match tool {
                        ResponsesApiNamespaceTool::Function(tool) => {
                            relax_incompatible_strict_tool(tool);
                        }
                    }
                }
            }
            ToolSpec::ToolSearch { .. }
            | ToolSpec::ImageGeneration { .. }
            | ToolSpec::WebSearch { .. }
            | ToolSpec::Freeform(_) => {}
        }
    }
}

fn relax_incompatible_strict_tool(tool: &mut ResponsesApiTool) {
    if tool.strict && !is_strict_compatible_tool_schema(&tool.parameters) {
        tool.strict = false;
    }
}

fn is_strict_compatible_tool_schema(schema: &JsonSchema) -> bool {
    schema_is_object(schema) && is_strict_compatible_schema(schema)
}

fn schema_is_object(schema: &JsonSchema) -> bool {
    match &schema.schema_type {
        Some(JsonSchemaType::Single(JsonSchemaPrimitiveType::Object)) => true,
        Some(JsonSchemaType::Multiple(types)) => types.contains(&JsonSchemaPrimitiveType::Object),
        Some(JsonSchemaType::Single(_)) | None => schema.properties.is_some(),
    }
}

fn is_strict_compatible_schema(schema: &JsonSchema) -> bool {
    if schema_is_object(schema) {
        let Some(properties) = schema.properties.as_ref() else {
            return false;
        };
        let Some(required) = schema.required.as_ref() else {
            return false;
        };
        if !matches!(
            schema.additional_properties,
            Some(AdditionalProperties::Boolean(false))
        ) || required.len() != properties.len()
            || properties.keys().any(|key| !required.contains(key))
        {
            return false;
        }
    }

    schema
        .properties
        .iter()
        .flat_map(|properties| properties.values())
        .all(is_strict_compatible_schema)
        && schema
            .items
            .as_deref()
            .is_none_or(is_strict_compatible_schema)
        && schema
            .any_of
            .iter()
            .chain(&schema.one_of)
            .chain(&schema.all_of)
            .flat_map(|schemas| schemas.iter())
            .all(is_strict_compatible_schema)
        && schema
            .defs
            .iter()
            .chain(&schema.definitions)
            .flat_map(|schemas| schemas.values())
            .all(is_strict_compatible_schema)
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
    let context = model_info
        .use_responses_lite
        .then_some(ReasoningContext::AllTurns);

    if effort.is_none() && summary.is_none() && context.is_none() {
        None
    } else {
        Some(Reasoning {
            effort,
            summary,
            context,
        })
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
