use std::path::Path;

use codex_app_server_protocol::CommandAction;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_app_server_protocol::CoreToolCallStatus;
use codex_app_server_protocol::DynamicToolCallStatus;
use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::McpToolCallStatus;
use codex_app_server_protocol::PatchApplyStatus;
use serde_json::Value;

use crate::ToolKind;
use crate::ToolStatus;

pub(super) fn command_presentation(command: &str, actions: &[CommandAction]) -> (ToolKind, String) {
    if let [action] = actions {
        match action {
            CommandAction::Read { path, .. } => {
                return (ToolKind::Read, compact_path(path.as_path()));
            }
            CommandAction::ListFiles { path, .. } => {
                return (
                    ToolKind::List,
                    path.clone().unwrap_or_else(|| ".".to_string()),
                );
            }
            CommandAction::Search { query, path, .. } => {
                let title = match (query, path) {
                    (Some(query), Some(path)) => format!("{query} in {path}"),
                    (Some(query), None) => query.clone(),
                    (None, Some(path)) => path.clone(),
                    (None, None) => command.to_string(),
                };
                return (ToolKind::Search, title);
            }
            CommandAction::Unknown { .. } => {}
        }
    }
    (ToolKind::Execute, command.to_string())
}

pub(super) fn classify_tool_name(name: &str) -> ToolKind {
    let leaf = name.rsplit(['/', ':']).next().unwrap_or(name);
    let normalized = leaf
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect::<String>();
    match normalized.as_str() {
        "bash" | "shell" | "shellcommand" | "exec" | "execcommand" | "writestdin" => {
            ToolKind::Execute
        }
        "read" | "readfile" | "viewimage" | "readmcpresource" => ToolKind::Read,
        "edit" | "write" | "applypatch" | "notebookedit" => ToolKind::Edit,
        "glob" | "list" | "listdir" | "ls" => ToolKind::List,
        "grep" | "rg" | "search" | "find" | "searchquery" => ToolKind::Search,
        "webfetch" | "open" => ToolKind::WebFetch,
        "websearch" | "imagequery" => ToolKind::WebSearch,
        "skill" => ToolKind::Skill,
        "spawnagent" | "sendmessage" | "sendinput" | "waitagent" | "wait" | "closeagent"
        | "resumeagent" => ToolKind::Collab,
        "imagegen" | "imagegeneration" | "generateimage" => ToolKind::Media,
        _ => ToolKind::Other,
    }
}

pub(super) fn tool_call_title(kind: ToolKind, tool: &str, arguments: &Value) -> String {
    let summary = summarize_tool_call(tool, arguments);
    match kind {
        ToolKind::Read | ToolKind::Edit => compact_path(Path::new(&summary)),
        _ => summary,
    }
}

fn summarize_tool_call(tool: &str, arguments: &Value) -> String {
    for key in [
        "path",
        "file_path",
        "query",
        "pattern",
        "command",
        "cmd",
        "url",
        "prompt",
        "task",
    ] {
        if let Some(value) = arguments.get(key).and_then(Value::as_str)
            && !value.trim().is_empty()
        {
            return value.trim().to_string();
        }
    }
    if let Some(value) = arguments.as_str()
        && !value.trim().is_empty()
    {
        return value.trim().to_string();
    }
    tool.to_string()
}

pub(super) fn edit_title(changes: &[FileUpdateChange]) -> String {
    match changes {
        [] => "Editing files".to_string(),
        [change] => compact_path(Path::new(&change.path)),
        changes => format!("{} files", changes.len()),
    }
}

pub(super) fn compact_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map_or_else(|| path.display().to_string(), str::to_string)
}

pub(super) fn command_status(status: &CommandExecutionStatus) -> ToolStatus {
    match status {
        CommandExecutionStatus::InProgress => ToolStatus::Running,
        CommandExecutionStatus::Completed => ToolStatus::Success,
        CommandExecutionStatus::Failed => ToolStatus::Failed,
        CommandExecutionStatus::Declined => ToolStatus::Declined,
    }
}

pub(super) fn patch_status(status: &PatchApplyStatus) -> ToolStatus {
    match status {
        PatchApplyStatus::InProgress => ToolStatus::Running,
        PatchApplyStatus::Completed => ToolStatus::Success,
        PatchApplyStatus::Failed => ToolStatus::Failed,
        PatchApplyStatus::Declined => ToolStatus::Declined,
    }
}

pub(super) fn mcp_status(status: &McpToolCallStatus) -> ToolStatus {
    match status {
        McpToolCallStatus::InProgress => ToolStatus::Running,
        McpToolCallStatus::Completed => ToolStatus::Success,
        McpToolCallStatus::Failed => ToolStatus::Failed,
    }
}

pub(super) fn dynamic_status(status: &DynamicToolCallStatus, success: Option<bool>) -> ToolStatus {
    match (status, success) {
        (DynamicToolCallStatus::InProgress, _) => ToolStatus::Running,
        (_, Some(false)) | (DynamicToolCallStatus::Failed, _) => ToolStatus::Failed,
        (DynamicToolCallStatus::Completed, _) => ToolStatus::Success,
    }
}

pub(super) fn core_tool_status(status: CoreToolCallStatus) -> ToolStatus {
    match status {
        CoreToolCallStatus::InProgress => ToolStatus::Running,
        CoreToolCallStatus::Completed => ToolStatus::Success,
        CoreToolCallStatus::Failed => ToolStatus::Failed,
        CoreToolCallStatus::Interrupted => ToolStatus::Interrupted,
    }
}

pub(super) fn status_from_text(status: &str) -> ToolStatus {
    match status.to_ascii_lowercase().as_str() {
        "in_progress" | "inprogress" | "running" => ToolStatus::Running,
        "failed" | "error" => ToolStatus::Failed,
        "declined" => ToolStatus::Declined,
        "interrupted" | "cancelled" | "canceled" => ToolStatus::Interrupted,
        _ => ToolStatus::Success,
    }
}
