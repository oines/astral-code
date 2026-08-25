use crate::client_common::Prompt;
use crate::responses_request;
use crate::responses_request::ResponsesRequestParams;
use crate::session::turn_context::TurnContext;
use crate::tools::hosted_spec::WebSearchToolOptions;
use crate::tools::hosted_spec::create_web_search_tool;
use codex_api::Reasoning;
use codex_api::ReasoningContext;
use codex_api::ResponsesApiRequest;
use codex_features::Feature;
use codex_protocol::account::PlanType;
use codex_protocol::account::ProviderAccount;
use codex_protocol::config_types::WebSearchMode;
use codex_protocol::openai_models::InputModality;
use codex_tools::AdditionalProperties;
use codex_tools::JsonSchema;
use codex_tools::JsonSchemaPrimitiveType;
use codex_tools::JsonSchemaType;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use codex_tools::provider_neutral_tool_name_for_tool_name;

const IMAGE_OUTPUT_FORMAT: &str = "png";

pub(super) fn build_responses_request(
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
    let mut prompt = prompt.clone();
    flatten_reserved_namespaces(&mut prompt);
    relax_incompatible_strict_tools(&mut prompt.tools);

    let mut request = responses_request::build_responses_request(ResponsesRequestParams {
        prompt: &prompt,
        model_info,
        effort,
        summary,
        service_tier,
        prompt_cache_key,
    })?;
    if model_info.use_responses_lite {
        request.parallel_tool_calls = false;
        match request.reasoning.as_mut() {
            Some(reasoning) => reasoning.context = Some(ReasoningContext::AllTurns),
            None => {
                request.reasoning = Some(Reasoning {
                    effort: None,
                    summary: None,
                    context: Some(ReasoningContext::AllTurns),
                });
            }
        }
    }
    Ok(request)
}

pub(super) fn hosted_model_tool_specs(turn_context: &TurnContext) -> Vec<ToolSpec> {
    if turn_context.model_info.use_responses_lite {
        return Vec::new();
    }

    let mut specs = Vec::new();
    if turn_context.model_info.supports_web_search
        && let Some(web_search) = create_web_search_tool(WebSearchToolOptions {
            web_search_mode: Some(turn_context.config.web_search_mode.value()),
            web_search_config: turn_context.config.web_search_config.as_ref(),
            web_search_tool_type: turn_context.model_info.web_search_tool_type,
        })
    {
        specs.push(web_search);
    }
    if image_generation_available(turn_context) {
        specs.push(ToolSpec::ImageGeneration {
            output_format: IMAGE_OUTPUT_FORMAT.to_string(),
        });
    }
    specs
}

pub(super) fn extension_tool_visible(turn_context: &TurnContext, tool_name: &ToolName) -> bool {
    match (tool_name.namespace.as_deref(), tool_name.name.as_str()) {
        (Some("web"), "search" | "fetch") => false,
        (Some("web"), "run") => {
            turn_context.model_info.use_responses_lite
                && turn_context.model_info.supports_web_search
                && turn_context.config.web_search_mode.value() != WebSearchMode::Disabled
                && turn_context
                    .features
                    .get()
                    .enabled(Feature::StandaloneWebSearch)
        }
        (Some("image_gen"), "imagegen") => {
            turn_context.model_info.use_responses_lite && image_generation_available(turn_context)
        }
        _ => true,
    }
}

fn image_generation_available(turn_context: &TurnContext) -> bool {
    if !turn_context.model_info.supports_image_generation
        || !turn_context
            .model_info
            .input_modalities
            .contains(&InputModality::Image)
        || !turn_context
            .features
            .get()
            .enabled(Feature::ImageGeneration)
    {
        return false;
    }

    !matches!(
        turn_context.provider.account_state().account,
        Some(ProviderAccount::Chatgpt {
            plan_type: PlanType::Free,
            ..
        })
    )
}

fn flatten_reserved_namespaces(prompt: &mut Prompt) {
    let mut projected = Vec::with_capacity(prompt.tools.len());
    for tool in prompt.tools.drain(..) {
        match tool {
            ToolSpec::Namespace(namespace) if namespace.name == "web" => {
                for tool in namespace.tools {
                    match tool {
                        ResponsesApiNamespaceTool::Function(mut tool) => {
                            tool.name = provider_neutral_tool_name_for_tool_name(
                                &ToolName::namespaced("web", tool.name),
                            );
                            projected.push(ToolSpec::Function(tool));
                        }
                    }
                }
            }
            tool => projected.push(tool),
        }
    }
    prompt.tools = projected;
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

#[cfg(test)]
#[path = "codex_tests.rs"]
mod tests;
