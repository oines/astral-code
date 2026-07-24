use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolPayload;

/// Returns the raw patch text carried by either the native freeform payload or
/// a function-only provider envelope.
pub(crate) fn apply_patch_input_from_payload(
    payload: &ToolPayload,
) -> Result<String, FunctionCallError> {
    match payload {
        ToolPayload::Custom { input } => Ok(input.clone()),
        ToolPayload::Function { arguments } => apply_patch_input_from_function_arguments(arguments),
        ToolPayload::ToolSearch { .. } => Err(FunctionCallError::RespondToModel(
            "apply_patch handler received unsupported payload".to_string(),
        )),
    }
}

fn apply_patch_input_from_function_arguments(arguments: &str) -> Result<String, FunctionCallError> {
    let value: serde_json::Value = serde_json::from_str(arguments).map_err(|err| {
        FunctionCallError::RespondToModel(format!("failed to parse apply_patch arguments: {err}"))
    })?;
    if let Some(input) = value.as_str() {
        return Ok(input.to_string());
    }
    if let Some(input) = value.get("input").and_then(serde_json::Value::as_str) {
        return Ok(input.to_string());
    }
    Err(FunctionCallError::RespondToModel(
        "apply_patch handler received invalid function arguments".to_string(),
    ))
}

#[cfg(test)]
#[path = "apply_patch_payload_tests.rs"]
mod tests;
