//! Typed presentation view for app-server client-hosted dynamic tool calls.

use codex_app_server_protocol::DynamicToolCallOutputContentItem;
use codex_app_server_protocol::DynamicToolCallStatus;
use codex_app_server_protocol::ThreadItem;
use serde_json::Value;

/// Borrow-only dynamic-tool view. It preserves the canonical app-server item
/// while keeping client-hosted tool semantics distinct from MCP calls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DynamicToolCallBlock<'a> {
    namespace: Option<&'a str>,
    tool: &'a str,
    arguments: &'a Value,
    status: &'a DynamicToolCallStatus,
    content_items: Option<&'a [DynamicToolCallOutputContentItem]>,
    success: Option<bool>,
    duration_ms: Option<i64>,
}

impl<'a> DynamicToolCallBlock<'a> {
    pub(crate) fn from_item(item: &'a ThreadItem) -> Option<Self> {
        let ThreadItem::DynamicToolCall {
            namespace,
            tool,
            arguments,
            status,
            content_items,
            success,
            duration_ms,
            ..
        } = item
        else {
            return None;
        };
        Some(Self {
            namespace: namespace.as_deref(),
            tool,
            arguments,
            status,
            content_items: content_items.as_deref(),
            success: *success,
            duration_ms: *duration_ms,
        })
    }

    pub fn namespace(self) -> Option<&'a str> {
        self.namespace
    }

    pub fn tool(self) -> &'a str {
        self.tool
    }

    pub fn arguments(self) -> &'a Value {
        self.arguments
    }

    pub fn content_items(self) -> &'a [DynamicToolCallOutputContentItem] {
        self.content_items.unwrap_or_default()
    }

    pub fn duration_ms(self) -> Option<i64> {
        self.duration_ms
    }

    pub fn running(self) -> bool {
        matches!(self.status, DynamicToolCallStatus::InProgress)
    }

    pub fn failed(self) -> bool {
        self.success == Some(false) || matches!(self.status, DynamicToolCallStatus::Failed)
    }

    pub fn has_details(self) -> bool {
        !self.content_items().is_empty() || (!self.is_web_fetch() && has_arguments(self.arguments))
    }

    pub fn is_web_fetch(self) -> bool {
        self.namespace == Some("web") && self.tool == "fetch"
    }

    pub fn web_fetch_url(self) -> Option<&'a str> {
        self.is_web_fetch()
            .then(|| self.arguments.get("url").and_then(Value::as_str))
            .flatten()
    }
}

fn has_arguments(arguments: &Value) -> bool {
    match arguments {
        Value::Null => false,
        Value::Object(arguments) => !arguments.is_empty(),
        Value::Array(arguments) => !arguments.is_empty(),
        Value::Bool(_) | Value::Number(_) | Value::String(_) => true,
    }
}
