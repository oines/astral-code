//! Client-hosted dynamic tool cards using Grok's WebFetch and generic tool
//! presentation without conflating their protocol semantics with MCP.

use codex_app_server_protocol::DynamicToolCallOutputContentItem;

use crate::DisplayMode;
use crate::DynamicToolCallBlock;
use crate::EntryDisplayState;
use crate::MarkdownLine;

use super::EntryRenderOptions;
use super::tool_card::ToolCardHeader;
use super::tool_card::ToolCardStatus;
use super::tool_card::append_section;
use super::tool_card::bounded_output_lines;
use super::tool_card::render_arguments;
use super::tool_card::render_body;
use super::tool_card::render_header;
use super::tool_card::titleize;

pub(super) fn render(
    call: DynamicToolCallBlock<'_>,
    state: EntryDisplayState,
    options: EntryRenderOptions,
) -> Vec<MarkdownLine> {
    let status = status(call);
    let (title, detail) = header_parts(call);
    let mut lines = render_header(
        ToolCardHeader {
            title,
            detail,
            status,
            duration_ms: call.duration_ms(),
        },
        state.mode(),
        options,
    );
    if state.mode() == DisplayMode::Collapsed {
        return lines;
    }

    let arguments = if call.is_web_fetch() {
        Vec::new()
    } else {
        render_arguments(call.arguments(), options)
    };
    let result = render_body(result_lines(call), status, options);
    append_section(&mut lines, arguments);
    append_section(&mut lines, result);
    lines
}

fn status(call: DynamicToolCallBlock<'_>) -> ToolCardStatus {
    if call.failed() {
        ToolCardStatus::Failed
    } else if call.running() {
        ToolCardStatus::Running
    } else {
        ToolCardStatus::Succeeded
    }
}

fn header_parts(call: DynamicToolCallBlock<'_>) -> (Option<String>, String) {
    if call.is_web_fetch() {
        return (
            Some("Fetch".to_string()),
            call.web_fetch_url().unwrap_or_default().to_string(),
        );
    }
    (
        call.namespace().map(display_namespace),
        titleize(call.tool()),
    )
}

fn display_namespace(namespace: &str) -> String {
    let segment = namespace
        .rsplit("__")
        .find(|segment| !segment.is_empty())
        .unwrap_or(namespace);
    titleize(segment)
}

fn result_lines(call: DynamicToolCallBlock<'_>) -> Vec<String> {
    call.content_items()
        .iter()
        .flat_map(|item| match item {
            DynamicToolCallOutputContentItem::InputText { text } => bounded_output_lines(text),
            DynamicToolCallOutputContentItem::InputImage { .. } => {
                vec!["<image output>".to_string()]
            }
        })
        .collect()
}
