//! Typed presentation view for the exact Claude-compatible Glob/Grep schemas.

use codex_app_server_protocol::CoreToolCallStatus;
use codex_app_server_protocol::ThreadItem;
use serde_json::Value;

const GLOB_TOOL_NAME: &str = "Glob";
const GREP_TOOL_NAME: &str = "Grep";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GrepOutputMode {
    Content,
    Files,
    Count,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchKind<'a> {
    Glob {
        pattern: &'a str,
        path: Option<&'a str>,
    },
    Grep {
        pattern: &'a str,
        path: Option<&'a str>,
        glob: Option<&'a str>,
        output_mode: GrepOutputMode,
        file_type: Option<&'a str>,
        ignore_case: bool,
        multiline: bool,
    },
}

/// Borrow-only search call. The enum is exact to Astral's built-in Glob and
/// Grep schemas; arbitrary MCP or custom tool names do not enter this path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SearchCall<'a> {
    kind: SearchKind<'a>,
    status: CoreToolCallStatus,
    result: Option<&'a str>,
    error: Option<&'a str>,
    duration_ms: Option<i64>,
}

impl<'a> SearchCall<'a> {
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
        let kind = match tool.as_str() {
            GLOB_TOOL_NAME => SearchKind::Glob {
                pattern: string_arg(arguments, "pattern").unwrap_or("…"),
                path: string_arg(arguments, "path"),
            },
            GREP_TOOL_NAME => SearchKind::Grep {
                pattern: string_arg(arguments, "pattern").unwrap_or("…"),
                path: string_arg(arguments, "path"),
                glob: string_arg(arguments, "glob"),
                output_mode: match string_arg(arguments, "output_mode") {
                    Some("content") => GrepOutputMode::Content,
                    Some("count") => GrepOutputMode::Count,
                    Some("files_with_matches") | Some(_) | None => GrepOutputMode::Files,
                },
                file_type: string_arg(arguments, "type")
                    .or_else(|| string_arg(arguments, "file_type")),
                ignore_case: bool_arg(arguments, "-i")
                    .or_else(|| bool_arg(arguments, "ignore_case"))
                    .unwrap_or(false),
                multiline: bool_arg(arguments, "multiline").unwrap_or(false),
            },
            _ => return None,
        };
        Some(Self {
            kind,
            status: *status,
            result: non_empty(result.as_deref()),
            error: non_empty(error.as_deref()),
            duration_ms: *duration_ms,
        })
    }

    pub(crate) fn kind(self) -> SearchKind<'a> {
        self.kind
    }

    pub(crate) fn status(self) -> CoreToolCallStatus {
        self.status
    }

    pub(crate) fn result(self) -> Option<&'a str> {
        self.result
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

    pub(crate) fn failure_text(self) -> Option<&'a str> {
        if self.failed() {
            self.error.or(self.result)
        } else {
            None
        }
    }
}

fn string_arg<'a>(arguments: &'a Value, key: &str) -> Option<&'a str> {
    arguments.get(key).and_then(Value::as_str)
}

fn bool_arg(arguments: &Value, key: &str) -> Option<bool> {
    arguments.get(key).and_then(Value::as_bool)
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.trim().is_empty())
}
