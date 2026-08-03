//! Typed presentation view for canonical app-server MCP tool calls.

use codex_app_server_protocol::McpToolCallResult;
use codex_app_server_protocol::McpToolCallStatus;
use codex_app_server_protocol::ThreadItem;
use serde_json::Value;

/// Borrow-only MCP call view. The app-server item remains the source of truth;
/// this type only gives the renderer an exact, stable shape to consume.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct McpToolCallBlock<'a> {
    server: &'a str,
    tool: &'a str,
    status: &'a McpToolCallStatus,
    arguments: &'a Value,
    mcp_app_resource_uri: Option<&'a str>,
    plugin_id: Option<&'a str>,
    result: Option<&'a McpToolCallResult>,
    error: Option<&'a str>,
    duration_ms: Option<i64>,
}

impl<'a> McpToolCallBlock<'a> {
    pub(crate) fn from_item(item: &'a ThreadItem) -> Option<Self> {
        let ThreadItem::McpToolCall {
            server,
            tool,
            status,
            arguments,
            mcp_app_resource_uri,
            plugin_id,
            result,
            error,
            duration_ms,
            ..
        } = item
        else {
            return None;
        };
        Some(Self {
            server,
            tool,
            status,
            arguments,
            mcp_app_resource_uri: mcp_app_resource_uri.as_deref(),
            plugin_id: plugin_id.as_deref(),
            result: result.as_deref(),
            error: error.as_ref().map(|error| error.message.as_str()),
            duration_ms: *duration_ms,
        })
    }

    pub fn server(self) -> &'a str {
        self.server
    }

    pub fn tool(self) -> &'a str {
        self.tool
    }

    pub fn arguments(self) -> &'a Value {
        self.arguments
    }

    pub fn mcp_app_resource_uri(self) -> Option<&'a str> {
        self.mcp_app_resource_uri
    }

    pub fn plugin_id(self) -> Option<&'a str> {
        self.plugin_id
    }

    pub fn result(self) -> Option<&'a McpToolCallResult> {
        self.result
    }

    pub fn error(self) -> Option<&'a str> {
        self.error
    }

    pub fn duration_ms(self) -> Option<i64> {
        self.duration_ms
    }

    pub fn running(self) -> bool {
        matches!(self.status, McpToolCallStatus::InProgress)
    }

    pub fn failed(self) -> bool {
        self.error.is_some() || matches!(self.status, McpToolCallStatus::Failed)
    }

    pub fn has_details(self) -> bool {
        has_arguments(self.arguments)
            || self.result.is_some_and(|result| {
                !result.content.is_empty() || result.structured_content.is_some()
            })
            || self.error.is_some()
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
