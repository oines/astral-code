use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolPayload;
use codex_protocol::models::SearchToolCallParams;
use codex_tools::AGENT_TOOL_NAME;
use codex_tools::ASK_USER_QUESTION_TOOL_NAME;
use codex_tools::BASH_TOOL_NAME;
use codex_tools::EDIT_TOOL_NAME;
use codex_tools::GLOB_TOOL_NAME;
use codex_tools::GREP_TOOL_NAME;
use codex_tools::LIST_MCP_RESOURCES_TOOL_NAME;
use codex_tools::MONITOR_TOOL_NAME;
use codex_tools::READ_MCP_RESOURCE_TOOL_NAME;
use codex_tools::READ_TOOL_NAME;
use codex_tools::REQUEST_PERMISSIONS_TOOL_NAME;
use codex_tools::SEND_MESSAGE_TOOL_NAME;
use codex_tools::TASK_STOP_TOOL_NAME;
use codex_tools::TODO_WRITE_TOOL_NAME;
use codex_tools::TOOL_SEARCH_FLAVOR_TOOL_NAME;
use codex_tools::TOOL_SEARCH_TOOL_NAME;
use codex_tools::ToolName;
use codex_tools::WRITE_TOOL_NAME;
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
        BASH_TOOL_NAME => rewrite_function_payload(payload, BASH_TOOL_NAME, rewrite_bash_args)?,
        READ_TOOL_NAME => rewrite_function_payload(payload, READ_TOOL_NAME, rewrite_read_args)?,
        WRITE_TOOL_NAME => rewrite_function_payload(payload, WRITE_TOOL_NAME, rewrite_write_args)?,
        EDIT_TOOL_NAME => rewrite_function_payload(payload, EDIT_TOOL_NAME, rewrite_edit_args)?,
        GLOB_TOOL_NAME => rewrite_function_payload(payload, GLOB_TOOL_NAME, rewrite_glob_args)?,
        GREP_TOOL_NAME => rewrite_function_payload(payload, GREP_TOOL_NAME, rewrite_grep_args)?,
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
        BASH_TOOL_NAME | READ_TOOL_NAME | WRITE_TOOL_NAME | EDIT_TOOL_NAME | GLOB_TOOL_NAME
        | GREP_TOOL_NAME => Some("exec_command"),
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

fn rewrite_bash_args(value: Value) -> Result<Value, FunctionCallError> {
    let mut object = expect_object(BASH_TOOL_NAME, value)?;
    move_field_if_absent(&mut object, "command", "cmd");
    move_field_if_absent(&mut object, "cwd", "workdir");
    move_field_if_absent(&mut object, "timeout", "timeout_ms");
    object.remove("run_in_background");
    Ok(Value::Object(object))
}

fn rewrite_read_args(value: Value) -> Result<Value, FunctionCallError> {
    let args: AstralReadArgs = serde_json::from_value(value).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to parse {READ_TOOL_NAME} arguments: {err}"
        ))
    })?;
    if args.pages.is_some() {
        return Err(FunctionCallError::RespondToModel(
            "Read pages are not supported yet; read PDFs through Bash or a dedicated reader"
                .to_string(),
        ));
    }

    Ok(exec_payload(shell_join(vec![
        "python3".to_string(),
        "-c".to_string(),
        READ_SCRIPT.to_string(),
        args.file_path,
        args.offset
            .map(|value| value.to_string())
            .unwrap_or_default(),
        args.limit
            .map(|value| value.to_string())
            .unwrap_or_default(),
    ])))
}

fn rewrite_write_args(value: Value) -> Result<Value, FunctionCallError> {
    let args: AstralWriteArgs = serde_json::from_value(value).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to parse {WRITE_TOOL_NAME} arguments: {err}"
        ))
    })?;

    Ok(exec_payload(shell_join(vec![
        "python3".to_string(),
        "-c".to_string(),
        WRITE_SCRIPT.to_string(),
        args.file_path,
        args.content,
    ])))
}

fn rewrite_edit_args(value: Value) -> Result<Value, FunctionCallError> {
    let args: AstralEditArgs = serde_json::from_value(value).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to parse {EDIT_TOOL_NAME} arguments: {err}"
        ))
    })?;

    Ok(exec_payload(shell_join(vec![
        "python3".to_string(),
        "-c".to_string(),
        EDIT_SCRIPT.to_string(),
        args.file_path,
        args.old_string,
        args.new_string,
        args.replace_all.to_string(),
    ])))
}

fn rewrite_glob_args(value: Value) -> Result<Value, FunctionCallError> {
    let args: AstralGlobArgs = serde_json::from_value(value).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to parse {GLOB_TOOL_NAME} arguments: {err}"
        ))
    })?;

    Ok(exec_payload(shell_join(vec![
        "python3".to_string(),
        "-c".to_string(),
        GLOB_SCRIPT.to_string(),
        args.path.unwrap_or_else(|| ".".to_string()),
        args.pattern,
    ])))
}

fn rewrite_grep_args(value: Value) -> Result<Value, FunctionCallError> {
    let args: AstralGrepArgs = serde_json::from_value(value).map_err(|err| {
        FunctionCallError::RespondToModel(format!(
            "failed to parse {GREP_TOOL_NAME} arguments: {err}"
        ))
    })?;

    Ok(exec_payload(shell_join(vec![
        "python3".to_string(),
        "-c".to_string(),
        GREP_SCRIPT.to_string(),
        serde_json::to_string(&args).map_err(|err| {
            FunctionCallError::Fatal(format!("failed to serialize {GREP_TOOL_NAME} args: {err}"))
        })?,
    ])))
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

fn exec_payload(cmd: String) -> Value {
    json!({ "cmd": cmd })
}

fn shell_join(args: Vec<String>) -> String {
    codex_shell_command::parse_command::shlex_join(&args)
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

const READ_SCRIPT: &str = r#"
from pathlib import Path
import sys
path = Path(sys.argv[1])
offset = int(sys.argv[2]) if sys.argv[2] else None
limit = int(sys.argv[3]) if sys.argv[3] else None
text = path.read_text(errors="replace")
lines = text.splitlines(True)
start = max(offset - 1, 0) if offset else 0
end = start + limit if limit else None
sys.stdout.write("".join(lines[start:end]))
"#;

const WRITE_SCRIPT: &str = r#"
from pathlib import Path
import sys
Path(sys.argv[1]).write_text(sys.argv[2])
"#;

const EDIT_SCRIPT: &str = r#"
from pathlib import Path
import sys
path = Path(sys.argv[1])
old = sys.argv[2]
new = sys.argv[3]
replace_all = sys.argv[4] == "true"
text = path.read_text()
count = text.count(old)
if count == 0:
    raise SystemExit("old_string not found")
if count > 1 and not replace_all:
    raise SystemExit("old_string appears multiple times; set replace_all to true")
path.write_text(text.replace(old, new) if replace_all else text.replace(old, new, 1))
"#;

const GLOB_SCRIPT: &str = r#"
from pathlib import Path
import sys
root = Path(sys.argv[1])
pattern = sys.argv[2]
matches = sorted(str(path) for path in root.glob(pattern))
sys.stdout.write("\n".join(matches))
if matches:
    sys.stdout.write("\n")
"#;

const GREP_SCRIPT: &str = r#"
import json
import subprocess
import sys
args = json.loads(sys.argv[1])
cmd = ["rg"]
mode = args.get("output_mode")
if mode == "files_with_matches":
    cmd.append("--files-with-matches")
elif mode == "count":
    cmd.append("--count")
if args.get("line_numbers"):
    cmd.append("--line-number")
if args.get("ignore_case"):
    cmd.append("--ignore-case")
if args.get("multiline"):
    cmd.append("--multiline")
for flag, key in (("-B", "before"), ("-A", "after"), ("-C", "context")):
    if args.get(key) is not None:
        cmd.extend([flag, str(args[key])])
if args.get("glob"):
    cmd.extend(["-g", args["glob"]])
if args.get("file_type"):
    cmd.extend(["-t", args["file_type"]])
cmd.append(args["pattern"])
cmd.append(args.get("path") or ".")
process = subprocess.run(cmd, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
lines = process.stdout.splitlines(True)
offset = args.get("offset") or 0
head_limit = args.get("head_limit")
selected = lines[offset:]
if head_limit is not None:
    selected = selected[:head_limit]
sys.stdout.write("".join(selected))
raise SystemExit(0 if process.returncode == 1 else process.returncode)
"#;

#[derive(Deserialize)]
struct AstralReadArgs {
    file_path: String,
    #[serde(default)]
    offset: Option<u64>,
    #[serde(default)]
    limit: Option<u64>,
    #[serde(default)]
    pages: Option<String>,
}

#[derive(Deserialize)]
struct AstralWriteArgs {
    file_path: String,
    content: String,
}

#[derive(Deserialize)]
struct AstralEditArgs {
    file_path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

#[derive(Deserialize)]
struct AstralGlobArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Deserialize, serde::Serialize)]
struct AstralGrepArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    glob: Option<String>,
    #[serde(default)]
    output_mode: Option<String>,
    #[serde(default, rename = "-B")]
    before: Option<u64>,
    #[serde(default, rename = "-A")]
    after: Option<u64>,
    #[serde(default, rename = "-C", alias = "context")]
    context: Option<u64>,
    #[serde(default, rename = "-n")]
    line_numbers: bool,
    #[serde(default, rename = "-i")]
    ignore_case: bool,
    #[serde(default, rename = "type")]
    file_type: Option<String>,
    #[serde(default)]
    head_limit: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    multiline: bool,
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
