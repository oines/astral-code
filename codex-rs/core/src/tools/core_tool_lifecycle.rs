use std::time::Duration;
use std::time::Instant;

use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use codex_protocol::items::CoreToolCallItem;
use codex_protocol::items::CoreToolCallStatus;
use codex_protocol::items::TurnItem;
use codex_tools::EDIT_TOOL_NAME;
use codex_tools::GLOB_TOOL_NAME;
use codex_tools::GREP_TOOL_NAME;
use codex_tools::LIST_BACKGROUND_TASKS_TOOL_NAME;
use codex_tools::READ_TASK_OUTPUT_TOOL_NAME;
use codex_tools::READ_TOOL_NAME;
use codex_tools::SEND_TASK_INPUT_TOOL_NAME;
use codex_tools::STOP_BACKGROUND_TASK_TOOL_NAME;
use codex_tools::TODO_WRITE_TOOL_NAME;
use codex_tools::ToolName;
use codex_tools::WRITE_TOOL_NAME;
use serde_json::Value;

const CORE_TOOL_RESULT_PREVIEW_MAX_BYTES: usize = 2 * 1024;

pub(crate) struct CoreToolCallTracker {
    started_at: Instant,
}

pub(crate) async fn maybe_emit_core_tool_started(
    invocation: &ToolInvocation,
) -> Option<CoreToolCallTracker> {
    if !should_emit_core_tool_call(&invocation.tool_name) {
        return None;
    }

    invocation
        .session
        .emit_turn_item_started(
            &invocation.turn,
            &TurnItem::CoreToolCall(core_tool_item(
                invocation,
                CoreToolCallStatus::InProgress,
                None,
                None,
                None,
            )),
        )
        .await;

    Some(CoreToolCallTracker {
        started_at: Instant::now(),
    })
}

pub(crate) async fn maybe_emit_core_tool_completed(
    tracker: Option<&CoreToolCallTracker>,
    invocation: &ToolInvocation,
    success: bool,
    preview: String,
) {
    let Some(tracker) = tracker else {
        return;
    };
    let status = if success {
        CoreToolCallStatus::Completed
    } else {
        CoreToolCallStatus::Failed
    };
    invocation
        .session
        .emit_turn_item_completed(
            &invocation.turn,
            TurnItem::CoreToolCall(core_tool_item(
                invocation,
                status,
                Some(bounded_preview(&preview)),
                None,
                Some(tracker.started_at.elapsed()),
            )),
        )
        .await;
}

pub(crate) async fn maybe_emit_core_tool_failed(
    tracker: Option<&CoreToolCallTracker>,
    invocation: &ToolInvocation,
    err: &FunctionCallError,
) {
    let Some(tracker) = tracker else {
        return;
    };
    invocation
        .session
        .emit_turn_item_completed(
            &invocation.turn,
            TurnItem::CoreToolCall(core_tool_item(
                invocation,
                CoreToolCallStatus::Failed,
                None,
                Some(bounded_preview(&err.to_string())),
                Some(tracker.started_at.elapsed()),
            )),
        )
        .await;
}

fn should_emit_core_tool_call(tool_name: &ToolName) -> bool {
    if tool_name.namespace.is_some() {
        return false;
    }

    matches!(
        tool_name.name.as_str(),
        READ_TOOL_NAME
            | WRITE_TOOL_NAME
            | EDIT_TOOL_NAME
            | GLOB_TOOL_NAME
            | GREP_TOOL_NAME
            | TODO_WRITE_TOOL_NAME
            | READ_TASK_OUTPUT_TOOL_NAME
            | SEND_TASK_INPUT_TOOL_NAME
            | LIST_BACKGROUND_TASKS_TOOL_NAME
            | STOP_BACKGROUND_TASK_TOOL_NAME
    )
}

fn core_tool_item(
    invocation: &ToolInvocation,
    status: CoreToolCallStatus,
    result: Option<String>,
    error: Option<String>,
    duration: Option<Duration>,
) -> CoreToolCallItem {
    CoreToolCallItem {
        id: invocation.call_id.clone(),
        tool: invocation.tool_name.name.clone(),
        arguments: payload_arguments(&invocation.payload),
        status,
        result,
        error,
        duration,
    }
}

fn payload_arguments(payload: &ToolPayload) -> Value {
    match payload {
        ToolPayload::Function { arguments } => {
            serde_json::from_str(arguments).unwrap_or_else(|_| Value::String(arguments.clone()))
        }
        ToolPayload::ToolSearch { arguments } => {
            serde_json::json!({ "query": arguments.query.clone() })
        }
        ToolPayload::Custom { input } => Value::String(input.clone()),
    }
}

fn bounded_preview(text: &str) -> String {
    if text.len() <= CORE_TOOL_RESULT_PREVIEW_MAX_BYTES {
        return text.to_string();
    }

    let mut end = CORE_TOOL_RESULT_PREVIEW_MAX_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[truncated]", &text[..end])
}
