use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolPayload;
use codex_tools::LIST_MCP_RESOURCES_TOOL_NAME;
use codex_tools::READ_MCP_RESOURCE_TOOL_NAME;
use codex_tools::ToolName;

pub(crate) fn canonical_astral_tool_name(tool_name: &ToolName) -> ToolName {
    let Some(name) = canonical_astral_plain_name(tool_name) else {
        return tool_name.clone();
    };

    ToolName::plain(name)
}

pub(crate) fn canonicalize_astral_tool_call(
    tool_name: ToolName,
    payload: ToolPayload,
) -> Result<(ToolName, ToolPayload), FunctionCallError> {
    let Some(target_name) = canonical_astral_plain_name(&tool_name) else {
        return Ok((tool_name, payload));
    };

    Ok((ToolName::plain(target_name), payload))
}

fn canonical_astral_plain_name(tool_name: &ToolName) -> Option<&'static str> {
    if tool_name.namespace.is_some() {
        return None;
    }

    match tool_name.name.as_str() {
        LIST_MCP_RESOURCES_TOOL_NAME => Some("list_mcp_resources"),
        READ_MCP_RESOURCE_TOOL_NAME => Some("read_mcp_resource"),
        _ => None,
    }
}

#[cfg(test)]
#[path = "astral_tool_bridge_tests.rs"]
mod tests;
