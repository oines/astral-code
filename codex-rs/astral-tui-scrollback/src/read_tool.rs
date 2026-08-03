//! Typed presentation view for the exact Claude-compatible `Read` schema.

use codex_app_server_protocol::CoreToolCallStatus;
use codex_app_server_protocol::ThreadItem;

const TOOL_NAME: &str = "Read";

/// Borrow-only view of one Read call. Other generic core tools stay available
/// for their own exact renderer instead of falling through a heuristic card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReadCall<'a> {
    path: &'a str,
    offset: Option<usize>,
    limit: Option<usize>,
    status: CoreToolCallStatus,
    result: Option<&'a str>,
    error: Option<&'a str>,
    duration_ms: Option<i64>,
}

impl<'a> ReadCall<'a> {
    pub(crate) fn from_item(item: &'a ThreadItem) -> Option<Self> {
        let ThreadItem::CoreToolCall {
            tool,
            arguments,
            status,
            result,
            error,
            duration_ms,
            ..
        } = item
        else {
            return None;
        };
        (tool == TOOL_NAME).then(|| Self {
            path: arguments
                .get("file_path")
                .and_then(|value| value.as_str())
                .unwrap_or("…"),
            offset: arguments
                .get("offset")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok()),
            limit: arguments
                .get("limit")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok()),
            status: *status,
            result: non_empty(result.as_deref()),
            error: non_empty(error.as_deref()),
            duration_ms: *duration_ms,
        })
    }

    pub(crate) fn path(self) -> &'a str {
        self.path
    }

    pub(crate) fn offset(self) -> Option<usize> {
        self.offset
    }

    pub(crate) fn limit(self) -> Option<usize> {
        self.limit
    }

    pub(crate) fn status(self) -> CoreToolCallStatus {
        self.status
    }

    pub(crate) fn result(self) -> Option<&'a str> {
        self.result
    }

    pub(crate) fn error(self) -> Option<&'a str> {
        self.error
    }

    pub(crate) fn duration_ms(self) -> Option<i64> {
        self.duration_ms
    }

    pub(crate) fn failed(self) -> bool {
        self.error.is_some()
            || matches!(
                self.status,
                CoreToolCallStatus::Failed | CoreToolCallStatus::Interrupted
            )
    }

    pub(crate) fn has_details(self) -> bool {
        self.result.is_some() && !self.empty() && !self.unchanged()
    }

    pub(crate) fn empty(self) -> bool {
        self.result.is_some_and(|result| {
            result.starts_with(
                "<system-reminder>Warning: the file exists but the contents are empty.",
            )
        })
    }

    pub(crate) fn unchanged(self) -> bool {
        self.result
            .is_some_and(|result| result.starts_with("File unchanged since last read"))
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.trim().is_empty())
}
