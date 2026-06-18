use crate::tools::context::ToolPayload;
use codex_tools::BASH_TOOL_NAME;
use codex_tools::ToolName;

pub(crate) fn canonical_astral_tool_name(tool_name: &ToolName) -> ToolName {
    if tool_name.namespace.is_some() {
        return tool_name.clone();
    }

    match tool_name.name.as_str() {
        "exec_command" | "shell_command" => ToolName::plain(BASH_TOOL_NAME),
        _ => tool_name.clone(),
    }
}

pub(crate) fn canonicalize_astral_tool_call(
    tool_name: ToolName,
    payload: ToolPayload,
) -> (ToolName, ToolPayload) {
    let payload = match tool_name.name.as_str() {
        "exec_command" => rewrite_function_payload_fields(
            payload,
            &[
                ("cmd", "command"),
                ("workdir", "cwd"),
                ("timeout_ms", "timeout"),
            ],
        ),
        "shell_command" => rewrite_function_payload_fields(
            payload,
            &[("workdir", "cwd"), ("timeout_ms", "timeout")],
        ),
        _ => payload,
    };
    (canonical_astral_tool_name(&tool_name), payload)
}

fn rewrite_function_payload_fields(
    payload: ToolPayload,
    fields: &[(&'static str, &'static str)],
) -> ToolPayload {
    let ToolPayload::Function { arguments } = payload else {
        return payload;
    };

    let Ok(serde_json::Value::Object(mut object)) =
        serde_json::from_str::<serde_json::Value>(&arguments)
    else {
        return ToolPayload::Function { arguments };
    };

    for (from, to) in fields {
        if object.contains_key(*to) {
            continue;
        }
        if let Some(value) = object.remove(*from) {
            object.insert((*to).to_string(), value);
        }
    }

    let arguments = serde_json::to_string(&object).unwrap_or(arguments);
    ToolPayload::Function { arguments }
}

#[cfg(test)]
#[path = "astral_tool_bridge_tests.rs"]
mod tests;
