use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolPayload;
use codex_protocol::models::SearchToolCallParams;
use codex_tools::AGENT_TOOL_NAME;
use codex_tools::ASK_USER_QUESTION_TOOL_NAME;
use codex_tools::LIST_MCP_RESOURCES_TOOL_NAME;
use codex_tools::MONITOR_TOOL_NAME;
use codex_tools::READ_MCP_RESOURCE_TOOL_NAME;
use codex_tools::REQUEST_PERMISSIONS_TOOL_NAME;
use codex_tools::SEND_MESSAGE_TOOL_NAME;
use codex_tools::TASK_STOP_TOOL_NAME;
use codex_tools::TODO_WRITE_TOOL_NAME;
use codex_tools::TOOL_SEARCH_FLAVOR_TOOL_NAME;
use codex_tools::TOOL_SEARCH_TOOL_NAME;
use codex_tools::ToolName;
use serde::Deserialize;
use serde_json::Map;
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
        MONITOR_TOOL_NAME => {
            rewrite_function_payload(payload, MONITOR_TOOL_NAME, rewrite_monitor_args)?
        }
        TODO_WRITE_TOOL_NAME => {
            rewrite_function_payload(payload, TODO_WRITE_TOOL_NAME, rewrite_todo_write_args)?
        }
        ASK_USER_QUESTION_TOOL_NAME => rewrite_function_payload(
            payload,
            ASK_USER_QUESTION_TOOL_NAME,
            rewrite_ask_user_question_args,
        )?,
        REQUEST_PERMISSIONS_TOOL_NAME => rewrite_function_payload(
            payload,
            REQUEST_PERMISSIONS_TOOL_NAME,
            rewrite_request_permissions_args,
        )?,
        TOOL_SEARCH_FLAVOR_TOOL_NAME => rewrite_tool_search_payload(payload)?,
        AGENT_TOOL_NAME => rewrite_function_payload(payload, AGENT_TOOL_NAME, rewrite_agent_args)?,
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
        MONITOR_TOOL_NAME => Some("write_stdin"),
        TODO_WRITE_TOOL_NAME => Some("update_plan"),
        ASK_USER_QUESTION_TOOL_NAME => Some("request_user_input"),
        REQUEST_PERMISSIONS_TOOL_NAME => Some("request_permissions"),
        TOOL_SEARCH_FLAVOR_TOOL_NAME => Some(TOOL_SEARCH_TOOL_NAME),
        LIST_MCP_RESOURCES_TOOL_NAME => Some("list_mcp_resources"),
        READ_MCP_RESOURCE_TOOL_NAME => Some("read_mcp_resource"),
        AGENT_TOOL_NAME => Some("spawn_agent"),
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

fn rewrite_monitor_args(value: Value) -> Result<Value, FunctionCallError> {
    let mut object = expect_object(MONITOR_TOOL_NAME, value)?;
    move_field_if_absent(&mut object, "task_id", "session_id");
    move_field_if_absent(&mut object, "shell_id", "session_id");
    Ok(Value::Object(object))
}

fn rewrite_todo_write_args(value: Value) -> Result<Value, FunctionCallError> {
    let args: AstralTodoWriteArgs = serde_json::from_value(value).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to parse {TODO_WRITE_TOOL_NAME} arguments: {err}"
        ))
    })?;
    let plan = args
        .todos
        .into_iter()
        .map(|todo| json!({ "step": todo.content, "status": todo.status }))
        .collect::<Vec<_>>();

    Ok(json!({
        "explanation": args.explanation,
        "plan": plan,
    }))
}

fn rewrite_ask_user_question_args(value: Value) -> Result<Value, FunctionCallError> {
    let args: AstralAskUserQuestionArgs = serde_json::from_value(value).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to parse {ASK_USER_QUESTION_TOOL_NAME} arguments: {err}"
        ))
    })?;

    let questions = args
        .questions
        .into_iter()
        .enumerate()
        .map(|(index, question)| {
            json!({
                "id": question.id.unwrap_or_else(|| format!("question_{}", index + 1)),
                "header": question.header,
                "question": question.question,
                "options": question.options,
            })
        })
        .collect::<Vec<_>>();

    Ok(json!({ "questions": questions }))
}

fn rewrite_request_permissions_args(value: Value) -> Result<Value, FunctionCallError> {
    let args: AstralRequestPermissionsArgs = serde_json::from_value(value).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to parse {REQUEST_PERMISSIONS_TOOL_NAME} arguments: {err}"
        ))
    })?;
    let permissions = args
        .input
        .get("additional_permissions")
        .or_else(|| args.input.get("additionalPermissions"))
        .or_else(|| args.input.get("permissions"))
        .cloned()
        .unwrap_or_else(|| json!({}));

    Ok(json!({
        "environment_id": args.environment_id,
        "reason": args.reason,
        "permissions": permissions,
    }))
}

fn rewrite_tool_search_payload(payload: ToolPayload) -> Result<ToolPayload, FunctionCallError> {
    match payload {
        ToolPayload::Function { arguments } => {
            let arguments: SearchToolCallParams =
                serde_json::from_str(&arguments).map_err(|err| {
                    FunctionCallError::RespondToModel(format!(
                        "failed to parse {TOOL_SEARCH_FLAVOR_TOOL_NAME} arguments: {err}"
                    ))
                })?;
            Ok(ToolPayload::ToolSearch { arguments })
        }
        ToolPayload::ToolSearch { .. } => Ok(payload),
        ToolPayload::Custom { .. } => Ok(payload),
    }
}

fn rewrite_agent_args(value: Value) -> Result<Value, FunctionCallError> {
    let args: AstralAgentArgs = serde_json::from_value(value).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to parse {AGENT_TOOL_NAME} arguments: {err}"
        ))
    })?;

    Ok(json!({
        "message": args.prompt,
        "task_name": args.description,
        "agent_type": args.subagent_type,
        "model": args.model,
        "reasoning_effort": args.reasoning_effort,
        "service_tier": args.service_tier,
        "fork_turns": args.fork_turns,
    }))
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

fn expect_object(tool_name: &str, value: Value) -> Result<Map<String, Value>, FunctionCallError> {
    match value {
        Value::Object(object) => Ok(object),
        _ => Err(FunctionCallError::RespondToModel(format!(
            "{tool_name} arguments must be a JSON object"
        ))),
    }
}

fn move_field_if_absent(object: &mut Map<String, Value>, from: &str, to: &str) {
    if !object.contains_key(to)
        && let Some(value) = object.remove(from)
    {
        object.insert(to.to_string(), value);
    }
}

#[derive(Deserialize)]
struct AstralTodoWriteArgs {
    todos: Vec<AstralTodoItem>,
    #[serde(default)]
    explanation: Option<String>,
}

#[derive(Deserialize)]
struct AstralTodoItem {
    content: String,
    status: String,
}

#[derive(Deserialize)]
struct AstralAskUserQuestionArgs {
    questions: Vec<AstralQuestion>,
}

#[derive(Deserialize)]
struct AstralQuestion {
    #[serde(default)]
    id: Option<String>,
    header: String,
    question: String,
    #[serde(default)]
    options: Option<Vec<AstralQuestionOption>>,
}

#[derive(Deserialize, serde::Serialize)]
struct AstralQuestionOption {
    label: String,
    description: String,
}

#[derive(Deserialize)]
struct AstralRequestPermissionsArgs {
    reason: String,
    input: Value,
    #[serde(default, rename = "environment_id", alias = "environmentId")]
    environment_id: Option<String>,
}

#[derive(Deserialize)]
struct AstralAgentArgs {
    description: String,
    prompt: String,
    #[serde(default)]
    subagent_type: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    service_tier: Option<String>,
    #[serde(default)]
    fork_turns: Option<String>,
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
