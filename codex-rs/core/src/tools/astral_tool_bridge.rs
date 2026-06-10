use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolPayload;
use codex_tools::LIST_MCP_RESOURCES_TOOL_NAME;
use codex_tools::READ_MCP_RESOURCE_TOOL_NAME;
use codex_tools::SEND_MESSAGE_TOOL_NAME;
use codex_tools::TASK_STOP_TOOL_NAME;
use codex_tools::ToolName;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

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

    let payload = match tool_name.name.as_str() {
        SEND_MESSAGE_TOOL_NAME => {
            rewrite_function_payload(payload, SEND_MESSAGE_TOOL_NAME, rewrite_send_message_args)?
        }
        TASK_STOP_TOOL_NAME => {
            rewrite_function_payload(payload, TASK_STOP_TOOL_NAME, rewrite_task_stop_args)?
        }
        LIST_MCP_RESOURCES_TOOL_NAME | READ_MCP_RESOURCE_TOOL_NAME => payload,
        _ => payload,
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
        SEND_MESSAGE_TOOL_NAME => Some("send_message"),
        TASK_STOP_TOOL_NAME => Some("interrupt_agent"),
        _ => None,
    }
}

fn rewrite_function_payload(
    payload: ToolPayload,
    tool_name: &str,
    rewrite: fn(Value) -> Result<Value, FunctionCallError>,
) -> Result<ToolPayload, FunctionCallError> {
    let ToolPayload::Function { arguments } = payload else {
        return Ok(payload);
    };

    let value = parse_json_arguments(tool_name, &arguments)?;
    let value = rewrite(value)?;
    Ok(ToolPayload::Function {
        arguments: serialize_json_arguments(tool_name, value)?,
    })
}

fn parse_json_arguments(tool_name: &str, arguments: &str) -> Result<Value, FunctionCallError> {
    serde_json::from_str(arguments).map_err(|err| {
        FunctionCallError::RespondToModel(format!("failed to parse {tool_name} arguments: {err}"))
    })
}

fn serialize_json_arguments(tool_name: &str, value: Value) -> Result<String, FunctionCallError> {
    serde_json::to_string(&value).map_err(|err| {
        FunctionCallError::Fatal(format!(
            "failed to serialize canonical {tool_name} arguments: {err}"
        ))
    })
}

fn rewrite_send_message_args(value: Value) -> Result<Value, FunctionCallError> {
    let args: AstralSendMessageArgs = serde_json::from_value(value).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to parse {SEND_MESSAGE_TOOL_NAME} arguments: {err}"
        ))
    })?;
    let message = match args.message {
        Value::String(message) => message,
        other => other.to_string(),
    };

    Ok(json!({
        "target": args.to,
        "message": message,
    }))
}

fn rewrite_task_stop_args(value: Value) -> Result<Value, FunctionCallError> {
    let args: AstralTaskStopArgs = serde_json::from_value(value).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to parse {TASK_STOP_TOOL_NAME} arguments: {err}"
        ))
    })?;
    let target = args.task_id.or(args.shell_id).ok_or_else(|| {
        FunctionCallError::RespondToModel(
            "TaskStop requires `task_id` or `shell_id` to identify the target".to_string(),
        )
    })?;

    Ok(json!({ "target": target }))
}

#[derive(Deserialize)]
struct AstralSendMessageArgs {
    to: String,
    message: Value,
}

#[derive(Deserialize)]
struct AstralTaskStopArgs {
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    shell_id: Option<String>,
}

#[cfg(test)]
#[path = "astral_tool_bridge_tests.rs"]
mod tests;
