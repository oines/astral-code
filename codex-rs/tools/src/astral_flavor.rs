use crate::astral_prompts;
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
        astral_prompts::bash_description(),
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
                bool_property(
                    "tty",
                    "Allocate a PTY for interactive commands that need follow-up input",
                ),
            ],
            ["command"],
        ),
    )
}

fn read_tool() -> AgentTool {
    tool(
        READ_TOOL_NAME,
        astral_prompts::read_description(),
        object(
            [
                string_property("file_path", "The absolute path to the file to read"),
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
        astral_prompts::write_description(),
        object(
            [
                string_property(
                    "file_path",
                    "The absolute path to the file to write; must be absolute",
                ),
                string_property("content", "The content to write to the file"),
            ],
            ["file_path", "content"],
        ),
    )
}

fn edit_tool() -> AgentTool {
    tool(
        EDIT_TOOL_NAME,
        astral_prompts::edit_description(),
        object(
            [
                string_property("file_path", "The absolute path to the file to modify"),
                string_property(
                    "old_string",
                    "The exact text to replace. CRITICAL: Never include any part of the line number prefix from the Read tool output in old_string.",
                ),
                string_property(
                    "new_string",
                    "The text to replace it with; must be different from old_string. CRITICAL: Never include any part of the line number prefix from the Read tool output in new_string.",
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
        astral_prompts::glob_description(),
        object(
            [
                string_property("pattern", "The glob pattern to match files against"),
                string_property(
                    "path",
                    "The directory to search in. If not specified, the current working directory will be used. IMPORTANT: Omit this field to use the default directory. DO NOT enter \"undefined\" or \"null\" - simply omit it for the default behavior. Must be a valid directory path if provided.",
                ),
            ],
            ["pattern"],
        ),
    )
}

fn grep_tool() -> AgentTool {
    tool(
        GREP_TOOL_NAME,
        astral_prompts::grep_description(),
        object(
            [
                string_property(
                    "pattern",
                    "The regular expression pattern to search for in file contents",
                ),
                string_property(
                    "path",
                    "File or directory to search in (rg PATH). Defaults to current working directory.",
                ),
                string_property(
                    "glob",
                    "Glob pattern to filter files (e.g. \"*.js\", \"*.{ts,tsx}\") - maps to rg --glob",
                ),
                enum_property(
                    "output_mode",
                    "Output mode: \"content\" shows matching lines (supports -A/-B/-C context, -n line numbers, head_limit), \"files_with_matches\" shows file paths (supports head_limit), \"count\" shows match counts (supports head_limit). Defaults to \"files_with_matches\".",
                    ["content", "files_with_matches", "count"],
                ),
                number_property(
                    "-B",
                    "Number of lines to show before each match (rg -B). Requires output_mode: \"content\", ignored otherwise.",
                ),
                number_property(
                    "-A",
                    "Number of lines to show after each match (rg -A). Requires output_mode: \"content\", ignored otherwise.",
                ),
                number_property("-C", "Alias for context."),
                number_property(
                    "context",
                    "Number of lines to show before and after each match (rg -C). Requires output_mode: \"content\", ignored otherwise.",
                ),
                bool_property(
                    "-n",
                    "Show line numbers in output (rg -n). Requires output_mode: \"content\", ignored otherwise. Defaults to true.",
                ),
                bool_property("-i", "Case insensitive search (rg -i)"),
                string_property(
                    "type",
                    "File type to search (rg --type). Common types: js, py, rust, go, java, etc. More efficient than include for standard file types.",
                ),
                number_property(
                    "head_limit",
                    "Limit output to first N lines/entries, equivalent to \"| head -N\". Works across all output modes: content (limits output lines), files_with_matches (limits file paths), count (limits count entries). Defaults to 250 when unspecified. Pass 0 for unlimited (use sparingly — large result sets waste context).",
                ),
                number_property(
                    "offset",
                    "Skip first N lines/entries before applying head_limit, equivalent to \"| tail -n +N | head -N\". Works across all output modes. Defaults to 0.",
                ),
                bool_property(
                    "multiline",
                    "Enable multiline mode where . matches newlines and patterns can span lines (rg -U --multiline-dotall). Default: false.",
                ),
            ],
            ["pattern"],
        ),
    )
}

fn todo_write_tool() -> AgentTool {
    tool(
        TODO_WRITE_TOOL_NAME,
        astral_prompts::todo_write_description(),
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
        astral_prompts::read_task_output_description(),
        object(
            [
                session_identifier_property(
                    "task_id",
                    "Background task_id returned by Bash or ListBackgroundTasks",
                ),
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
        astral_prompts::send_task_input_description(),
        object(
            [
                session_identifier_property(
                    "task_id",
                    "Background task_id returned by Bash or ListBackgroundTasks",
                ),
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
        astral_prompts::list_background_tasks_description(),
        object([], []),
    )
}

fn stop_background_task_tool() -> AgentTool {
    tool(
        STOP_BACKGROUND_TASK_TOOL_NAME,
        astral_prompts::stop_background_task_description(),
        object(
            [session_identifier_property(
                "task_id",
                "Background task_id returned by Bash or ListBackgroundTasks",
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
        astral_prompts::request_permissions_description(),
        object(
            [
                permission_profile_property(
                    "permissions",
                    "Exact filesystem or network permissions needed for the blocked action",
                ),
                string_property(
                    "reason",
                    "Brief reason the exact filesystem or network permissions are needed",
                ),
                string_property(
                    "tool_name",
                    "Optional source tool name for compatibility; permissions controls the actual request",
                ),
                json_property(
                    "input",
                    "Optional original blocked tool input for compatibility; prefer direct permissions",
                ),
            ],
            ["permissions", "reason"],
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

fn permission_profile_property(
    name: &'static str,
    description: &'static str,
) -> (&'static str, Value) {
    (
        name,
        json!({
            "type": "object",
            "description": description,
            "properties": {
                "file_system": {
                    "type": "object",
                    "description": "Filesystem permissions needed for the blocked action",
                    "properties": {
                        "read": {
                            "type": "array",
                            "description": "Absolute paths to grant read access",
                            "items": { "type": "string" }
                        },
                        "write": {
                            "type": "array",
                            "description": "Absolute paths to grant write access",
                            "items": { "type": "string" }
                        }
                    },
                    "additionalProperties": false
                },
                "network": {
                    "type": "object",
                    "description": "Network permissions needed for the blocked action",
                    "properties": {
                        "enabled": {
                            "type": "boolean",
                            "description": "True requests network access"
                        }
                    },
                    "additionalProperties": false
                }
            },
            "additionalProperties": false
        }),
    )
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
