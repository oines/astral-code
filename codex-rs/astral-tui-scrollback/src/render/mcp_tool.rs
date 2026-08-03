//! MCP tool card derived from Grok Build's `UseToolCallBlock` at commit
//! `47348d13ec4508dcfe440e34c6d511bb02998fb2` (Apache-2.0).

use serde_json::Value;

use crate::DisplayMode;
use crate::EntryDisplayState;
use crate::MarkdownLine;
use crate::McpToolCallBlock;

use super::EntryRenderOptions;
use super::tool_card::ToolCardHeader;
use super::tool_card::ToolCardStatus;
use super::tool_card::append_section;
use super::tool_card::bounded_output_lines;
use super::tool_card::bounded_output_value;
use super::tool_card::render_arguments;
use super::tool_card::render_body;
use super::tool_card::render_header;
use super::tool_card::titleize;

pub(super) fn render(
    call: McpToolCallBlock<'_>,
    state: EntryDisplayState,
    options: EntryRenderOptions,
) -> Vec<MarkdownLine> {
    let status = status(call);
    let mut lines = render_header(
        ToolCardHeader {
            title: (!call.server().is_empty()).then(|| titleize(call.server())),
            detail: titleize(call.tool()),
            status,
            duration_ms: call.duration_ms(),
        },
        state.mode(),
        options,
    );
    if state.mode() == DisplayMode::Collapsed {
        return lines;
    }

    let arguments = render_arguments(call.arguments(), options);
    let result = render_body(result_lines(call), status, options);
    append_section(&mut lines, arguments);
    append_section(&mut lines, result);
    lines
}

fn status(call: McpToolCallBlock<'_>) -> ToolCardStatus {
    if call.failed() {
        ToolCardStatus::Failed
    } else if call.running() {
        ToolCardStatus::Running
    } else {
        ToolCardStatus::Succeeded
    }
}

fn result_lines(call: McpToolCallBlock<'_>) -> Vec<String> {
    let mut lines = call
        .result()
        .into_iter()
        .flat_map(|result| result.content.iter())
        .flat_map(content_text)
        .collect::<Vec<_>>();
    if lines.is_empty()
        && let Some(structured) = call
            .result()
            .and_then(|result| result.structured_content.as_ref())
    {
        lines.push(format!(
            "structured result: {}",
            bounded_output_value(structured)
        ));
    }
    if let Some(error) = call.error() {
        lines.push(format!("Error: {error}"));
    }
    lines
}

fn content_text(content: &Value) -> Vec<String> {
    let text = match content.get("type").and_then(Value::as_str) {
        Some("text") => content
            .get("text")
            .and_then(Value::as_str)
            .map_or_else(|| content.to_string(), str::to_string),
        Some("image") => "<image content>".to_string(),
        Some("audio") => "<audio content>".to_string(),
        Some("resource") => content
            .pointer("/resource/uri")
            .and_then(Value::as_str)
            .map_or_else(
                || content.to_string(),
                |uri| format!("embedded resource: {uri}"),
            ),
        Some("resource_link") => content
            .get("uri")
            .and_then(Value::as_str)
            .map_or_else(|| content.to_string(), |uri| format!("link: {uri}")),
        Some(_) | None => content.to_string(),
    };
    bounded_output_lines(&text)
}
