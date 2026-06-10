use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolPayload;
use codex_tools::ToolName;

pub(crate) fn canonical_astral_tool_name(tool_name: &ToolName) -> ToolName {
    tool_name.clone()
}

pub(crate) fn canonicalize_astral_tool_call(
    tool_name: ToolName,
    payload: ToolPayload,
) -> Result<(ToolName, ToolPayload), FunctionCallError> {
    Ok((tool_name, payload))
}

#[cfg(test)]
#[path = "astral_tool_bridge_tests.rs"]
mod tests;
