use pretty_assertions::assert_eq;
use serde_json::json;

use super::ASTRAL_CORE_TOOL_NAMES;
use super::BASH_TOOL_NAME;
use super::EDIT_TOOL_NAME;
use super::GREP_TOOL_NAME;
use super::MONITOR_TOOL_NAME;
use super::READ_TOOL_NAME;
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
                "timeout": { "type": "number", "description": "Optional timeout in milliseconds" },
                "description": { "type": "string", "description": "Clear, concise description of what this command does in active voice" },
                "environment_id": { "type": "string", "description": "Optional target execution environment id when multiple environments exist" },
                "run_in_background": { "type": "boolean", "description": "Set to true to run this command in the background and read output later" }
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
        edit.input_schema["required"],
        json!(["file_path", "old_string", "new_string"])
    );
    assert_eq!(grep.input_schema["required"], json!(["pattern"]));
    assert_eq!(
        grep.input_schema["properties"]["output_mode"]["enum"],
        json!(["content", "files_with_matches", "count"])
    );
}

#[test]
fn todo_write_uses_legacy_task_list_shape() {
    let tool = astral_core_tool_by_name(TODO_WRITE_TOOL_NAME).expect("TodoWrite exists");

    assert_eq!(tool.input_schema["required"], json!(["todos"]));
    assert_eq!(
        tool.input_schema["properties"]["todos"]["items"]["properties"]["status"]["enum"],
        json!(["pending", "in_progress", "completed"])
    );
}

#[test]
fn monitor_uses_running_session_shape() {
    let tool = astral_core_tool_by_name(MONITOR_TOOL_NAME).expect("Monitor exists");

    assert_eq!(tool.input_schema["required"], json!(["session_id"]));
    assert_eq!(
        tool.input_schema["properties"]["session_id"]["type"],
        json!("integer")
    );
}

#[test]
fn unknown_tool_name_is_not_exposed() {
    assert_eq!(astral_core_tool_by_name("CronCreate"), None);
}
