use pretty_assertions::assert_eq;
use serde_json::json;

use super::ASTRAL_CORE_TOOL_NAMES;
use super::BASH_TOOL_NAME;
use super::EDIT_TOOL_NAME;
use super::GREP_TOOL_NAME;
use super::MONITOR_TOOL_NAME;
use super::READ_TOOL_NAME;
use super::SKILL_TOOL_NAME;
use super::TODO_WRITE_TOOL_NAME;
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
                "environment_id": { "type": "string", "description": "Optional target execution environment id when multiple environments exist" }
            },
            "required": ["command"],
            "additionalProperties": false
        })
    );
}

#[test]
fn file_and_search_tools_expose_expected_required_fields() {
    let read = astral_core_tool_by_name(READ_TOOL_NAME).expect("Read tool exists");
    let edit = astral_core_tool_by_name(EDIT_TOOL_NAME).expect("Edit tool exists");
    let grep = astral_core_tool_by_name(GREP_TOOL_NAME).expect("Grep tool exists");

    assert_eq!(read.input_schema["required"], json!(["file_path"]));
    assert_eq!(
        read.input_schema["properties"]["pages"]["type"],
        json!("string")
    );
    assert_eq!(
        edit.input_schema["required"],
        json!(["file_path", "old_string", "new_string"])
    );
    assert_eq!(grep.input_schema["required"], json!(["pattern"]));
    assert_eq!(
        grep.input_schema["properties"]["output_mode"]["enum"],
        json!(["content", "files_with_matches", "count"])
    );
    assert_eq!(
        grep.input_schema["properties"]["-n"]["description"],
        json!("Show line numbers in content output; defaults to true in content mode")
    );
    assert_eq!(
        grep.input_schema["properties"]["head_limit"]["description"],
        json!(
            "Limit output to first N lines or entries; defaults to 250, pass 0 for the maximum bounded output"
        )
    );
}

#[test]
fn todo_write_uses_claudeish_task_list_shape() {
    let tool = astral_core_tool_by_name(TODO_WRITE_TOOL_NAME).expect("TodoWrite exists");

    assert_eq!(tool.input_schema["required"], json!(["todos"]));
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
fn monitor_uses_running_session_shape() {
    let tool = astral_core_tool_by_name(MONITOR_TOOL_NAME).expect("Monitor exists");

    assert_eq!(tool.input_schema["required"], json!([]));
    assert_eq!(
        tool.input_schema["properties"]["session_id"]["anyOf"],
        json!([{ "type": "integer" }, { "type": "string" }])
    );
    assert_eq!(
        tool.input_schema["properties"]["shell_id"]["anyOf"],
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
