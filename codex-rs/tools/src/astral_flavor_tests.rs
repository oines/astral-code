use pretty_assertions::assert_eq;
use serde_json::json;

use super::ASTRAL_CORE_TOOL_NAMES;
use super::BASH_TOOL_NAME;
use super::EDIT_TOOL_NAME;
use super::GLOB_TOOL_NAME;
use super::GREP_TOOL_NAME;
use super::LIST_BACKGROUND_TASKS_TOOL_NAME;
use super::READ_TASK_OUTPUT_TOOL_NAME;
use super::READ_TOOL_NAME;
use super::REQUEST_PERMISSIONS_TOOL_NAME;
use super::SEND_TASK_INPUT_TOOL_NAME;
use super::SKILL_TOOL_NAME;
use super::STOP_BACKGROUND_TASK_TOOL_NAME;
use super::TODO_WRITE_TOOL_NAME;
use super::WRITE_TOOL_NAME;
use super::astral_core_tool_by_name;
use super::astral_core_tools;

#[test]
fn core_tools_follow_declared_order_and_names() {
    let tools = astral_core_tools();
    let names = tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(names, ASTRAL_CORE_TOOL_NAMES);
}

#[test]
fn bash_schema_uses_claudeish_command_shape() {
    let tool = astral_core_tool_by_name(BASH_TOOL_NAME).expect("Bash tool exists");

    assert_eq!(tool.name, "Bash");
    assert!(
        tool.description
            .contains("Executes a given bash command and returns its output.")
    );
    assert!(
        tool.description
            .contains("File search: Use Glob (NOT find or ls)")
    );
    assert!(
        tool.description
            .contains("Content search: Use Grep (NOT grep or rg)")
    );
    assert!(
        tool.description
            .contains("Read files: Use Read (NOT cat/head/tail)")
    );
    assert!(
        tool.description
            .contains("After starting a background command, use ReadTaskOutput")
    );
    assert!(
        tool.description
            .contains("use SendTaskInput to send exact stdin bytes")
    );
    assert!(tool.description.contains("use ListBackgroundTasks"));
    assert!(tool.description.contains("use StopBackgroundTask"));
    assert!(
        tool.description
            .contains("If a command is blocked by the sandbox or environment permission policy")
    );
    assert!(
        tool.description
            .contains("call RequestPermissions for the required access")
    );
    assert!(!tool.description.contains("dangerouslyDisableSandbox"));
    assert!(!tool.description.contains("Monitor"));
    assert_eq!(
        tool.input_schema,
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The command to execute" },
                "timeout": { "type": "number", "description": "Optional maximum command runtime in milliseconds" },
                "description": { "type": "string", "description": "Clear, concise description of what this command does in active voice" },
                "cwd": { "type": "string", "description": "Working directory for the command; omit to use the turn cwd" },
                "yield_time_ms": { "type": "integer", "description": "Milliseconds to wait for initial output before returning" },
                "max_output_tokens": { "type": "integer", "description": "Maximum output tokens to return" },
                "run_in_background": { "type": "boolean", "description": "Set true for long-running commands that should keep running while you monitor output separately" },
                "tty": { "type": "boolean", "description": "Allocate a PTY for interactive commands that need follow-up input" }
            },
            "required": ["command"],
            "additionalProperties": false
        })
    );
}

#[test]
fn request_permissions_schema_guides_exact_permission_recovery() {
    let tool =
        astral_core_tool_by_name(REQUEST_PERMISSIONS_TOOL_NAME).expect("RequestPermissions exists");

    assert_eq!(tool.name, "RequestPermissions");
    assert!(
        tool.description
            .contains("Request exact filesystem or network permissions")
    );
    assert!(tool.description.contains("Wait for approval"));
    assert!(tool.description.contains("retry the original action"));
    assert!(
        tool.description
            .contains("does not execute the blocked action")
    );
    assert_eq!(
        tool.input_schema["required"],
        json!(["permissions", "reason"])
    );
    assert_eq!(
        tool.input_schema["properties"]["permissions"]["type"],
        json!("object")
    );
    assert_eq!(
        tool.input_schema["properties"]["permissions"]["properties"]["file_system"]["properties"]["read"]
            ["items"]["type"],
        json!("string")
    );
    assert_eq!(
        tool.input_schema["properties"]["permissions"]["properties"]["file_system"]["properties"]["write"]
            ["items"]["type"],
        json!("string")
    );
    assert_eq!(
        tool.input_schema["properties"]["permissions"]["properties"]["network"]["properties"]["enabled"]
            ["type"],
        json!("boolean")
    );
    assert!(tool.input_schema["properties"]["environment_id"].is_null());
}

#[test]
fn file_and_search_tools_expose_expected_required_fields() {
    let read = astral_core_tool_by_name(READ_TOOL_NAME).expect("Read tool exists");
    let edit = astral_core_tool_by_name(EDIT_TOOL_NAME).expect("Edit tool exists");
    let glob = astral_core_tool_by_name(GLOB_TOOL_NAME).expect("Glob tool exists");
    let grep = astral_core_tool_by_name(GREP_TOOL_NAME).expect("Grep tool exists");
    let write = astral_core_tool_by_name(WRITE_TOOL_NAME).expect("Write tool exists");

    assert_eq!(read.input_schema["required"], json!(["file_path"]));
    assert!(
        read.description
            .contains("Reads a file from the local filesystem.")
    );
    assert!(
        read.description
            .contains("Results are returned using cat -n format, with line numbers starting at 1")
    );
    assert!(
        read.description
            .contains("If the user provides a path to a screenshot, ALWAYS use this tool")
    );
    assert!(!read.description.contains("PDF"));
    assert!(!read.description.contains("Notebook"));
    assert!(!read.description.contains("Jupyter"));
    assert!(
        write
            .description
            .contains("Writes a file to the local filesystem.")
    );
    assert!(
        write
            .description
            .contains("Prefer the Edit tool for modifying existing files")
    );
    assert!(
        edit.description.contains(
            "The line number prefix format is: line number + tab. Everything after that is the actual file content to match. Never include any part of the line number prefix in the old_string or new_string."
        )
    );
    assert!(edit.description.contains(
        "You must use your `Read` tool at least once in the conversation before editing."
    ));
    assert!(
        glob.description
            .contains("Fast file pattern matching tool that works with any codebase size")
    );
    assert!(
        grep.description
            .contains("A powerful search tool built on ripgrep")
    );
    assert!(grep.description.contains(
        "ALWAYS use Grep for search tasks. NEVER invoke `grep` or `rg` as a Bash command."
    ));
    assert!(read.input_schema["properties"]["pages"].is_null());
    assert!(read.input_schema["properties"]["environment_id"].is_null());
    assert!(write.input_schema["properties"]["environment_id"].is_null());
    assert!(edit.input_schema["properties"]["environment_id"].is_null());
    assert_eq!(
        edit.input_schema["required"],
        json!(["file_path", "old_string", "new_string"])
    );
    assert_eq!(glob.input_schema["required"], json!(["pattern"]));
    assert!(glob.input_schema["properties"]["environment_id"].is_null());
    assert_eq!(
        glob.input_schema["properties"]["path"]["description"],
        json!(
            "The directory to search in. If not specified, the current working directory will be used. IMPORTANT: Omit this field to use the default directory. DO NOT enter \"undefined\" or \"null\" - simply omit it for the default behavior. Must be a valid directory path if provided."
        )
    );
    assert_eq!(grep.input_schema["required"], json!(["pattern"]));
    assert!(grep.input_schema["properties"]["environment_id"].is_null());
    assert_eq!(
        grep.input_schema["properties"]["output_mode"]["enum"],
        json!(["content", "files_with_matches", "count"])
    );
    assert_eq!(
        grep.input_schema["properties"]["-n"]["description"],
        json!(
            "Show line numbers in output (rg -n). Requires output_mode: \"content\", ignored otherwise. Defaults to true."
        )
    );
    assert_eq!(
        grep.input_schema["properties"]["head_limit"]["description"],
        json!(
            "Limit output to first N lines/entries, equivalent to \"| head -N\". Works across all output modes: content (limits output lines), files_with_matches (limits file paths), count (limits count entries). Defaults to 250 when unspecified. Pass 0 for unlimited (use sparingly — large result sets waste context)."
        )
    );
    assert_eq!(
        grep.input_schema["properties"]["offset"]["description"],
        json!(
            "Skip first N lines/entries before applying head_limit, equivalent to \"| tail -n +N | head -N\". Works across all output modes. Defaults to 0."
        )
    );
}

#[test]
fn todo_write_uses_claudeish_task_list_shape() {
    let tool = astral_core_tool_by_name(TODO_WRITE_TOOL_NAME).expect("TodoWrite exists");

    assert!(
        tool.description
            .contains("Use this tool to create and manage a structured task list")
    );
    assert!(
        tool.description
            .contains("Mark it as in_progress BEFORE beginning work")
    );
    assert!(
        tool.description
            .contains("activeForm: The present continuous form shown during execution")
    );
    assert!(
        tool.description
            .contains("Exactly ONE task must be in_progress at any time")
    );
    assert_eq!(tool.input_schema["required"], json!(["todos"]));
    assert_eq!(
        tool.input_schema["properties"]
            .as_object()
            .expect("TodoWrite properties should be an object")
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["todos".to_string()]
    );
    assert_eq!(
        tool.input_schema["properties"]["todos"]["items"]["required"],
        json!(["content", "status", "activeForm"])
    );
    assert_eq!(
        tool.input_schema["properties"]["todos"]["items"]["properties"]["status"]["enum"],
        json!(["pending", "in_progress", "completed"])
    );
}

#[test]
fn background_task_tools_use_task_id_shape() {
    let read = astral_core_tool_by_name(READ_TASK_OUTPUT_TOOL_NAME).expect("ReadTaskOutput exists");
    let send = astral_core_tool_by_name(SEND_TASK_INPUT_TOOL_NAME).expect("SendTaskInput exists");
    let list = astral_core_tool_by_name(LIST_BACKGROUND_TASKS_TOOL_NAME)
        .expect("ListBackgroundTasks exists");
    let stop = astral_core_tool_by_name(STOP_BACKGROUND_TASK_TOOL_NAME)
        .expect("StopBackgroundTask exists");

    assert_eq!(read.input_schema["required"], json!(["task_id"]));
    assert_eq!(
        read.input_schema["properties"]["task_id"]["anyOf"],
        json!([{ "type": "integer" }, { "type": "string" }])
    );
    assert_eq!(send.input_schema["required"], json!(["task_id", "input"]));
    assert_eq!(
        send.input_schema["properties"]["input"]["type"],
        json!("string")
    );
    assert_eq!(list.input_schema["required"], json!([]));
    assert_eq!(stop.input_schema["required"], json!(["task_id"]));
    assert_eq!(
        stop.input_schema["properties"]["task_id"]["anyOf"],
        json!([{ "type": "integer" }, { "type": "string" }])
    );
}

#[test]
fn skill_uses_claudeish_skill_and_args_shape() {
    let tool = astral_core_tool_by_name(SKILL_TOOL_NAME).expect("Skill exists");

    assert_eq!(tool.input_schema["required"], json!(["skill"]));
    assert_eq!(
        tool.input_schema["properties"]["skill"]["type"],
        json!("string")
    );
    assert_eq!(
        tool.input_schema["properties"]["args"]["type"],
        json!("string")
    );
}

#[test]
fn unknown_tool_name_is_not_exposed() {
    assert_eq!(astral_core_tool_by_name("CronCreate"), None);
}
