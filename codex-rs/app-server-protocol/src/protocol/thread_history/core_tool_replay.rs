use codex_protocol::models::FunctionCallOutputPayload;

use crate::protocol::v2::CoreToolCallStatus;
use crate::protocol::v2::ThreadItem;

const RESULT_PREVIEW_MAX_BYTES: usize = 2 * 1024;

/// Reconstructs the provider-surface core tools that emit live
/// `CoreToolCall` items but persist only their function call and output.
///
/// The exact names mirror `core::tools::core_tool_lifecycle`; `update_plan`
/// additionally has a typed live notification and needs the same replay form.
pub(super) fn start(
    tool: &str,
    namespace: Option<&str>,
    arguments: &str,
    call_id: &str,
) -> Option<ThreadItem> {
    if namespace.is_some() || !is_replayable(tool) {
        return None;
    }
    Some(ThreadItem::CoreToolCall {
        id: call_id.to_string(),
        tool: tool.to_string(),
        arguments: serde_json::from_str(arguments)
            .unwrap_or_else(|_| serde_json::Value::String(arguments.to_string())),
        status: CoreToolCallStatus::InProgress,
        result: None,
        error: None,
        duration_ms: None,
    })
}

pub(super) fn complete(item: &mut ThreadItem, output: &FunctionCallOutputPayload) {
    let ThreadItem::CoreToolCall {
        status,
        result,
        error,
        ..
    } = item
    else {
        return;
    };
    *status = if output.success == Some(false) {
        CoreToolCallStatus::Failed
    } else {
        CoreToolCallStatus::Completed
    };
    *result = Some(bounded_preview(&output.to_string()));
    *error = None;
}

pub(super) fn interrupt_pending(items: &mut [ThreadItem]) {
    for item in items {
        if let ThreadItem::CoreToolCall { status, .. } = item
            && *status == CoreToolCallStatus::InProgress
        {
            *status = CoreToolCallStatus::Interrupted;
        }
    }
}

fn is_replayable(tool: &str) -> bool {
    matches!(
        tool,
        "Read"
            | "Write"
            | "Edit"
            | "Glob"
            | "Grep"
            | "TodoWrite"
            | "ReadTaskOutput"
            | "SendTaskInput"
            | "ListBackgroundTasks"
            | "StopBackgroundTask"
            | "update_plan"
    )
}

fn bounded_preview(text: &str) -> String {
    if text.len() <= RESULT_PREVIEW_MAX_BYTES {
        return text.to_string();
    }
    let mut end = RESULT_PREVIEW_MAX_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[truncated]", &text[..end])
}
