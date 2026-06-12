use codex_agent_protocol::AgentTool;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;

pub const ASK_USER_QUESTION_TOOL_NAME: &str = "AskUserQuestion";
pub const BASH_TOOL_NAME: &str = "Bash";
pub const EDIT_TOOL_NAME: &str = "Edit";
pub const GLOB_TOOL_NAME: &str = "Glob";
pub const GREP_TOOL_NAME: &str = "Grep";
pub const LIST_MCP_RESOURCES_TOOL_NAME: &str = "ListMcpResourcesTool";
pub const LIST_BACKGROUND_TASKS_TOOL_NAME: &str = "ListBackgroundTasks";
pub const READ_MCP_RESOURCE_TOOL_NAME: &str = "ReadMcpResourceTool";
pub const READ_TASK_OUTPUT_TOOL_NAME: &str = "ReadTaskOutput";
pub const READ_TOOL_NAME: &str = "Read";
pub const REQUEST_PERMISSIONS_TOOL_NAME: &str = "RequestPermissions";
pub const SEND_TASK_INPUT_TOOL_NAME: &str = "SendTaskInput";
pub const SKILL_TOOL_NAME: &str = "Skill";
pub const STOP_BACKGROUND_TASK_TOOL_NAME: &str = "StopBackgroundTask";
pub const TODO_WRITE_TOOL_NAME: &str = "TodoWrite";
pub const TOOL_SEARCH_FLAVOR_TOOL_NAME: &str = "ToolSearch";
pub const WRITE_TOOL_NAME: &str = "Write";

pub const ASTRAL_CORE_TOOL_NAMES: &[&str] = &[
    ASK_USER_QUESTION_TOOL_NAME,
    BASH_TOOL_NAME,
    EDIT_TOOL_NAME,
    GLOB_TOOL_NAME,
    GREP_TOOL_NAME,
    LIST_BACKGROUND_TASKS_TOOL_NAME,
    LIST_MCP_RESOURCES_TOOL_NAME,
    READ_MCP_RESOURCE_TOOL_NAME,
    READ_TASK_OUTPUT_TOOL_NAME,
    READ_TOOL_NAME,
    REQUEST_PERMISSIONS_TOOL_NAME,
    SEND_TASK_INPUT_TOOL_NAME,
    SKILL_TOOL_NAME,
    STOP_BACKGROUND_TASK_TOOL_NAME,
    TODO_WRITE_TOOL_NAME,
    TOOL_SEARCH_FLAVOR_TOOL_NAME,
    WRITE_TOOL_NAME,
];

pub fn astral_core_tools() -> Vec<AgentTool> {
    ASTRAL_CORE_TOOL_NAMES
        .iter()
        .filter_map(|name| astral_core_tool_by_name(name))
        .collect()
}

pub fn astral_core_tool_by_name(name: &str) -> Option<AgentTool> {
    match name {
        ASK_USER_QUESTION_TOOL_NAME => Some(ask_user_question_tool()),
        BASH_TOOL_NAME => Some(bash_tool()),
        EDIT_TOOL_NAME => Some(edit_tool()),
        GLOB_TOOL_NAME => Some(glob_tool()),
        GREP_TOOL_NAME => Some(grep_tool()),
        LIST_BACKGROUND_TASKS_TOOL_NAME => Some(list_background_tasks_tool()),
        LIST_MCP_RESOURCES_TOOL_NAME => Some(list_mcp_resources_tool()),
        READ_MCP_RESOURCE_TOOL_NAME => Some(read_mcp_resource_tool()),
        READ_TASK_OUTPUT_TOOL_NAME => Some(read_task_output_tool()),
        READ_TOOL_NAME => Some(read_tool()),
        REQUEST_PERMISSIONS_TOOL_NAME => Some(request_permissions_tool()),
        SEND_TASK_INPUT_TOOL_NAME => Some(send_task_input_tool()),
        SKILL_TOOL_NAME => Some(skill_tool()),
        STOP_BACKGROUND_TASK_TOOL_NAME => Some(stop_background_task_tool()),
        TODO_WRITE_TOOL_NAME => Some(todo_write_tool()),
        TOOL_SEARCH_FLAVOR_TOOL_NAME => Some(tool_search_tool()),
        WRITE_TOOL_NAME => Some(write_tool()),
        _ => None,
    }
}

fn bash_tool() -> AgentTool {
    tool(
        BASH_TOOL_NAME,
        "Execute a shell command through Astral's sandboxed PTY runtime.",
        object(
            [
                string_property("command", "The command to execute"),
                number_property(
                    "timeout",
                    "Optional maximum command runtime in milliseconds",
                ),
                string_property(
                    "description",
                    "Clear, concise description of what this command does in active voice",
                ),
                string_property(
                    "cwd",
                    "Working directory for the command; omit to use the turn cwd",
                ),
                integer_property(
                    "yield_time_ms",
                    "Milliseconds to wait for initial output before returning",
                ),
                integer_property("max_output_tokens", "Maximum output tokens to return"),
                bool_property(
                    "run_in_background",
                    "Set true for long-running commands that should keep running while you monitor output separately",
                ),
                string_property(
                    "environment_id",
                    "Optional target execution environment id when multiple environments exist",
                ),
            ],
            ["command"],
        ),
    )
}

fn read_tool() -> AgentTool {
    tool(
        READ_TOOL_NAME,
        "Read a text file or image from the active execution environment. Text output uses cat -n style line numbers.",
        object(
            [
                string_property("file_path", "The absolute path to the file to read"),
                environment_id_property(),
                integer_property(
                    "offset",
                    "The line number to start reading from; only provide for large files",
                ),
                integer_property(
                    "limit",
                    "The number of lines to read; only provide for large files",
                ),
            ],
            ["file_path"],
        ),
    )
}

fn write_tool() -> AgentTool {
    tool(
        WRITE_TOOL_NAME,
        "Create or overwrite a file in the active execution environment.",
        object(
            [
                string_property(
                    "file_path",
                    "The absolute path to the file to write; must be absolute",
                ),
                environment_id_property(),
                string_property("content", "The content to write to the file"),
            ],
            ["file_path", "content"],
        ),
    )
}

fn edit_tool() -> AgentTool {
    tool(
        EDIT_TOOL_NAME,
        "Edit a file in the active execution environment by replacing exact text.",
        object(
            [
                string_property("file_path", "The absolute path to the file to modify"),
                environment_id_property(),
                string_property("old_string", "The text to replace"),
                string_property(
                    "new_string",
                    "The text to replace it with; must be different from old_string",
                ),
                bool_property(
                    "replace_all",
                    "Replace all occurrences of old_string; defaults to false",
                ),
            ],
            ["file_path", "old_string", "new_string"],
        ),
    )
}

fn glob_tool() -> AgentTool {
    tool(
        GLOB_TOOL_NAME,
        "Find files by glob pattern, sorted by modification time.",
        object(
            [
                string_property("pattern", "The glob pattern to match files against"),
                string_property(
                    "path",
                    "Directory to search in; omit to use the current working directory",
                ),
                environment_id_property(),
            ],
            ["pattern"],
        ),
    )
}

fn grep_tool() -> AgentTool {
    tool(
        GREP_TOOL_NAME,
        "Search file contents with ripgrep-compatible options.",
        object(
            [
                string_property("pattern", "The regular expression pattern to search for"),
                string_property("path", "File or directory to search in; defaults to cwd"),
                environment_id_property(),
                string_property("glob", "Glob pattern to filter files"),
                enum_property(
                    "output_mode",
                    "Output mode; defaults to files_with_matches",
                    ["content", "files_with_matches", "count"],
                ),
                integer_property("-B", "Number of lines to show before each match"),
                integer_property("-A", "Number of lines to show after each match"),
                integer_property("-C", "Number of context lines before and after each match"),
                integer_property("context", "Alias for -C context lines"),
                bool_property(
                    "-n",
                    "Show line numbers in content output; defaults to true in content mode",
                ),
                bool_property("-i", "Case-insensitive search"),
                string_property("type", "File type to search, such as rust, py, js, go"),
                integer_property(
                    "head_limit",
                    "Limit output to first N lines or entries; defaults to 250, pass 0 for the maximum bounded output",
                ),
                integer_property("offset", "Skip first N lines or entries before limiting"),
                bool_property("multiline", "Enable multiline mode"),
            ],
            ["pattern"],
        ),
    )
}

fn todo_write_tool() -> AgentTool {
    tool(
        TODO_WRITE_TOOL_NAME,
        "Update the session task checklist.",
        object(
            [array_property(
                "todos",
                "The updated todo list",
                json!({
                    "type": "object",
                    "properties": {
                        "content": { "type": "string", "description": "Task description" },
                        "status": {
                            "type": "string",
                            "enum": ["pending", "in_progress", "completed"],
                            "description": "Task status"
                        },
                        "activeForm": {
                            "type": "string",
                            "description": "Short present-tense label for the active task"
                        },
                    },
                    "required": ["content", "status", "activeForm"],
                    "additionalProperties": false
                }),
            )],
            ["todos"],
        ),
    )
}

fn skill_tool() -> AgentTool {
    tool(
        SKILL_TOOL_NAME,
        "Load and execute a project, user, or plugin skill by name.",
        object(
            [
                string_property(
                    "skill",
                    "Skill name to invoke; omit any leading slash unless the name itself contains one",
                ),
                string_property("args", "Optional arguments or task context for the skill"),
            ],
            ["skill"],
        ),
    )
}

fn read_task_output_tool() -> AgentTool {
    tool(
        READ_TASK_OUTPUT_TOOL_NAME,
        "Read or poll output from a running background task.",
        object(
            [
                session_identifier_property("task_id", "The background task id returned by Bash"),
                integer_property(
                    "yield_time_ms",
                    "Milliseconds to wait for fresh output before returning",
                ),
                integer_property("max_output_tokens", "Maximum output tokens to return"),
            ],
            ["task_id"],
        ),
    )
}

fn send_task_input_tool() -> AgentTool {
    tool(
        SEND_TASK_INPUT_TOOL_NAME,
        "Send interactive stdin to a running background task, such as y\\n for a confirmation prompt.",
        object(
            [
                session_identifier_property("task_id", "The background task id returned by Bash"),
                string_property(
                    "input",
                    "Exact stdin bytes to send; include a trailing newline when pressing Enter is intended",
                ),
                integer_property(
                    "yield_time_ms",
                    "Milliseconds to wait for output after sending input",
                ),
                integer_property("max_output_tokens", "Maximum output tokens to return"),
            ],
            ["task_id", "input"],
        ),
    )
}

fn list_background_tasks_tool() -> AgentTool {
    tool(
        LIST_BACKGROUND_TASKS_TOOL_NAME,
        "List running background tasks and their task ids.",
        object([], []),
    )
}

fn stop_background_task_tool() -> AgentTool {
    tool(
        STOP_BACKGROUND_TASK_TOOL_NAME,
        "Stop a running background task by task id.",
        object(
            [session_identifier_property(
                "task_id",
                "The background task id returned by Bash or ListBackgroundTasks",
            )],
            ["task_id"],
        ),
    )
}

fn ask_user_question_tool() -> AgentTool {
    let option_schema = json!({
        "type": "object",
        "properties": {
            "label": { "type": "string", "description": "Concise option label shown to the user" },
            "description": { "type": "string", "description": "Explanation of this option's tradeoff or impact" },
            "preview": { "type": "string", "description": "Optional preview content for this option" }
        },
        "required": ["label", "description"],
        "additionalProperties": false
    });
    let question_schema = json!({
        "type": "object",
        "properties": {
            "question": { "type": "string", "description": "The complete question to ask the user" },
            "header": { "type": "string", "description": "Very short label displayed as a chip" },
            "options": { "type": "array", "minItems": 2, "maxItems": 4, "items": option_schema },
            "multiSelect": { "type": "boolean", "description": "Allow selecting multiple options" }
        },
        "required": ["question", "header", "options"],
        "additionalProperties": false
    });

    tool(
        ASK_USER_QUESTION_TOOL_NAME,
        "Ask the user one or more structured clarification questions.",
        object(
            [array_property(
                "questions",
                "Questions to ask the user; usually one, maximum four",
                question_schema,
            )],
            ["questions"],
        ),
    )
}

fn request_permissions_tool() -> AgentTool {
    tool(
        REQUEST_PERMISSIONS_TOOL_NAME,
        "Request elevated permission for a blocked action.",
        object(
            [
                string_property("tool_name", "The tool requiring permission"),
                json_property("input", "The original tool input that was blocked"),
                json_property(
                    "permissions",
                    "Permission profile to request directly when no blocked tool input is available",
                ),
                string_property(
                    "environment_id",
                    "Optional target execution environment id when multiple environments exist",
                ),
                string_property("reason", "Brief reason permission is needed"),
            ],
            ["reason"],
        ),
    )
}

fn tool_search_tool() -> AgentTool {
    tool(
        TOOL_SEARCH_FLAVOR_TOOL_NAME,
        "Search and load deferred tools.",
        object(
            [
                string_property("query", "Search query for tools"),
                integer_property(
                    "max_results",
                    "Maximum number of tools to return; defaults to 5",
                ),
                integer_property(
                    "limit",
                    "Compatibility alias for max_results; prefer max_results",
                ),
            ],
            ["query"],
        ),
    )
}

fn list_mcp_resources_tool() -> AgentTool {
    tool(
        LIST_MCP_RESOURCES_TOOL_NAME,
        "List resources exposed by connected MCP servers.",
        object([string_property("server", "Optional MCP server name")], []),
    )
}

fn read_mcp_resource_tool() -> AgentTool {
    tool(
        READ_MCP_RESOURCE_TOOL_NAME,
        "Read a resource exposed by a connected MCP server.",
        object(
            [
                string_property("server", "MCP server name"),
                string_property("uri", "Resource URI"),
            ],
            ["server", "uri"],
        ),
    )
}

fn tool(name: &str, description: &str, input_schema: Value) -> AgentTool {
    AgentTool {
        name: name.to_string(),
        description: description.to_string(),
        input_schema,
        metadata: BTreeMap::new(),
    }
}

fn object<const P: usize, const R: usize>(
    properties: [(&'static str, Value); P],
    required: [&'static str; R],
) -> Value {
    let mut property_map = Map::new();
    for (name, schema) in properties {
        property_map.insert(name.to_string(), schema);
    }

    json!({
        "type": "object",
        "properties": property_map,
        "required": required.as_slice(),
        "additionalProperties": false
    })
}

fn string_property(name: &'static str, description: &'static str) -> (&'static str, Value) {
    (
        name,
        json!({ "type": "string", "description": description }),
    )
}

fn environment_id_property() -> (&'static str, Value) {
    string_property(
        "environment_id",
        "Optional target execution environment id when multiple environments exist",
    )
}

fn number_property(name: &'static str, description: &'static str) -> (&'static str, Value) {
    (
        name,
        json!({ "type": "number", "description": description }),
    )
}

fn integer_property(name: &'static str, description: &'static str) -> (&'static str, Value) {
    (
        name,
        json!({ "type": "integer", "description": description }),
    )
}

fn bool_property(name: &'static str, description: &'static str) -> (&'static str, Value) {
    (
        name,
        json!({ "type": "boolean", "description": description }),
    )
}

fn json_property(name: &'static str, description: &'static str) -> (&'static str, Value) {
    (name, json!({ "description": description }))
}

fn session_identifier_property(
    name: &'static str,
    description: &'static str,
) -> (&'static str, Value) {
    (
        name,
        json!({
            "anyOf": [{ "type": "integer" }, { "type": "string" }],
            "description": description
        }),
    )
}

fn enum_property<const N: usize>(
    name: &'static str,
    description: &'static str,
    values: [&'static str; N],
) -> (&'static str, Value) {
    (
        name,
        json!({ "type": "string", "enum": values.as_slice(), "description": description }),
    )
}

fn array_property(
    name: &'static str,
    description: &'static str,
    items: Value,
) -> (&'static str, Value) {
    (
        name,
        json!({ "type": "array", "description": description, "items": items }),
    )
}

#[cfg(test)]
#[path = "astral_flavor_tests.rs"]
mod tests;
