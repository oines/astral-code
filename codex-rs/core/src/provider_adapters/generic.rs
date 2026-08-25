use crate::responses_request;
use crate::responses_request::ResponsesRequestParams;
use crate::session::turn_context::TurnContext;
use codex_api::ResponsesApiRequest;
use codex_protocol::config_types::WebSearchMode;
use codex_tools::ToolName;
use codex_tools::ToolSpec;

pub(super) fn build_responses_request(
    params: ResponsesRequestParams<'_>,
) -> codex_protocol::error::Result<ResponsesApiRequest> {
    responses_request::build_responses_request(params)
}

fn provider_neutral_web_tools_enabled(turn_context: &TurnContext) -> bool {
    turn_context.config.web_search_mode.value() == WebSearchMode::Live
}

pub(super) fn hosted_model_tool_specs() -> Vec<ToolSpec> {
    Vec::new()
}

pub(super) fn extension_tool_visible(turn_context: &TurnContext, tool_name: &ToolName) -> bool {
    if is_provider_neutral_web_tool(tool_name) {
        return provider_neutral_web_tools_enabled(turn_context);
    }
    !matches!(
        (tool_name.namespace.as_deref(), tool_name.name.as_str()),
        (Some("web"), "run") | (Some("image_gen"), "imagegen")
    )
}

fn is_provider_neutral_web_tool(tool_name: &ToolName) -> bool {
    matches!(
        (tool_name.namespace.as_deref(), tool_name.name.as_str()),
        (Some("web"), "search" | "fetch")
    )
}

#[cfg(test)]
#[path = "generic_tests.rs"]
mod tests;
