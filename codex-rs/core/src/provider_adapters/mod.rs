//! Provider-owned projections for model requests and tool surfaces.
//!
//! Keep dispatch here narrow. Generic providers use the standard wire behavior;
//! providers with materially different semantics own those differences in a
//! sibling module.

mod codex;
mod generic;

use crate::responses_request::ResponsesRequestParams;
use crate::session::turn_context::TurnContext;
use codex_api::ResponsesApiRequest;
use codex_model_provider::ModelProvider;
use codex_model_provider_info::ManagedAuthKind;
use codex_tools::ToolName;
use codex_tools::ToolSpec;

fn is_codex(provider: &dyn ModelProvider) -> bool {
    provider.info().managed_auth == Some(ManagedAuthKind::CodexOAuth)
}

pub(crate) fn build_responses_request(
    provider: &dyn ModelProvider,
    params: ResponsesRequestParams<'_>,
) -> codex_protocol::error::Result<ResponsesApiRequest> {
    if is_codex(provider) {
        codex::build_responses_request(params)
    } else {
        generic::build_responses_request(params)
    }
}

pub(crate) fn hosted_model_tool_specs(turn_context: &TurnContext) -> Vec<ToolSpec> {
    if is_codex(turn_context.provider.as_ref()) {
        codex::hosted_model_tool_specs(turn_context)
    } else {
        generic::hosted_model_tool_specs()
    }
}

pub(crate) fn extension_tool_visible(turn_context: &TurnContext, tool_name: &ToolName) -> bool {
    if is_codex(turn_context.provider.as_ref()) {
        codex::extension_tool_visible(turn_context, tool_name)
    } else {
        generic::extension_tool_visible(turn_context, tool_name)
    }
}
