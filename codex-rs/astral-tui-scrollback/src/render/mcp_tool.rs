//! MCP tool card derived from Grok Build's `UseToolCallBlock` at commit
//! `47348d13ec4508dcfe440e34c6d511bb02998fb2` (Apache-2.0).

use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use serde_json::Value;

use crate::DisplayMode;
use crate::EntryDisplayState;
use crate::LineJoiner;
use crate::MarkdownLine;
use crate::McpToolCallBlock;
use crate::render_literal_with_metadata;
use crate::wrap_styled_line_with_metadata;

use super::EntryRenderOptions;
use super::format_elapsed;
use super::prefix_lines;
use super::truncate_with_ellipsis;

const MAX_ARGUMENTS: usize = 12;
const MAX_ARGUMENT_CHARS: usize = 512;
const MAX_OUTPUT_LINES: usize = 10;
const MAX_OUTPUT_CHARS: usize = 4_096;

pub(super) fn render(
    call: McpToolCallBlock<'_>,
    state: EntryDisplayState,
    options: EntryRenderOptions,
) -> Vec<MarkdownLine> {
    let mut lines = render_header(call, state.mode(), options);
    if state.mode() == DisplayMode::Collapsed {
        return lines;
    }

    let arguments = render_arguments(call.arguments(), options);
    let result = render_result(call, options);
    if !arguments.is_empty() {
        lines.push(markdown_line(Line::default()));
        lines.extend(arguments);
    }
    if !result.is_empty() {
        lines.push(markdown_line(Line::default()));
        lines.extend(result);
    }
    lines
}

fn render_header(
    call: McpToolCallBlock<'_>,
    mode: DisplayMode,
    options: EntryRenderOptions,
) -> Vec<MarkdownLine> {
    let marker = if call.failed() {
        "× ".red()
    } else if call.running() {
        "◇ ".magenta()
    } else {
        "◆ ".green()
    };
    let mut title = Line::default();
    if !call.server().is_empty() {
        title.push_span(titleize(call.server()).bold().dim());
        title.push_span(" ");
    }
    title.push_span(Span::styled(
        titleize(call.tool()),
        Style::default().fg(options.diff_style.path),
    ));
    if !call.running()
        && let Some(duration_ms) = call.duration_ms().filter(|duration| *duration >= 0)
    {
        title.push_span(format!("  {}", format_elapsed(duration_ms)).dim());
    }
    let prefix_width = u16::try_from(Line::from(marker.clone()).width()).unwrap_or(u16::MAX);
    let mut lines =
        wrap_styled_line_with_metadata(&title, options.width.saturating_sub(prefix_width).max(1));
    prefix_lines(
        &mut lines,
        Line::from(marker),
        Line::from(" ".repeat(usize::from(prefix_width))),
    );
    if mode == DisplayMode::Collapsed {
        let wrapped = lines.len() > 1;
        lines.truncate(1);
        if wrapped && let Some(line) = lines.first_mut() {
            truncate_with_ellipsis(line, options.width);
        }
    }
    lines
}

fn render_arguments(arguments: &Value, options: EntryRenderOptions) -> Vec<MarkdownLine> {
    match arguments {
        Value::Null => Vec::new(),
        Value::Object(arguments) => {
            let mut lines = arguments
                .iter()
                .take(MAX_ARGUMENTS)
                .flat_map(|(key, value)| render_argument(key, value, options))
                .collect::<Vec<_>>();
            if arguments.len() > MAX_ARGUMENTS {
                lines.push(markdown_line(Line::from(
                    format!("  … {} more arguments", arguments.len() - MAX_ARGUMENTS).dim(),
                )));
            }
            lines
        }
        Value::Array(_) | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            render_argument("arguments", arguments, options)
        }
    }
}

fn render_argument(key: &str, value: &Value, options: EntryRenderOptions) -> Vec<MarkdownLine> {
    let value = bounded_value(value, MAX_ARGUMENT_CHARS);
    let mut lines = Vec::new();
    for (index, value_line) in value.split('\n').enumerate() {
        let line = if index == 0 {
            Line::from(vec![
                format!("{key}: ").dim(),
                value_line.to_string().into(),
            ])
        } else {
            Line::from(value_line.to_string())
        };
        let mut wrapped =
            wrap_styled_line_with_metadata(&line, options.width.saturating_sub(2).max(1));
        prefix_lines(&mut wrapped, Line::from("  "), Line::from("  "));
        lines.extend(wrapped);
    }
    lines
}

fn render_result(call: McpToolCallBlock<'_>, options: EntryRenderOptions) -> Vec<MarkdownLine> {
    let mut source_lines = call
        .result()
        .into_iter()
        .flat_map(|result| result.content.iter())
        .flat_map(content_text)
        .collect::<Vec<_>>();
    if source_lines.is_empty()
        && let Some(structured) = call
            .result()
            .and_then(|result| result.structured_content.as_ref())
    {
        source_lines.push(format!(
            "structured result: {}",
            bounded_value(structured, MAX_OUTPUT_CHARS)
        ));
    }
    if let Some(error) = call.error() {
        source_lines.push(format!("Error: {error}"));
    }
    if source_lines.is_empty() {
        return Vec::new();
    }

    let hidden = source_lines.len().saturating_sub(MAX_OUTPUT_LINES);
    source_lines.truncate(MAX_OUTPUT_LINES);
    if hidden > 0 {
        source_lines.push(format!("… {hidden} hidden lines"));
    }
    let source = truncate_chars(&source_lines.join("\n"), MAX_OUTPUT_CHARS);
    let style = if call.failed() {
        Style::default().red()
    } else {
        Style::default()
    };
    let mut lines =
        render_literal_with_metadata(&source, options.width.saturating_sub(4).max(1), style);
    prefix_lines(
        &mut lines,
        Line::from("  │ ".dim()),
        Line::from("  │ ".dim()),
    );
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
    truncate_chars(&text, MAX_OUTPUT_CHARS)
        .split('\n')
        .map(str::to_string)
        .collect()
}

fn bounded_value(value: &Value, max_chars: usize) -> String {
    let value = value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_string);
    truncate_chars(&value, max_chars)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut truncated = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}

fn titleize(name: &str) -> String {
    name.split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn markdown_line(line: Line<'static>) -> MarkdownLine {
    MarkdownLine {
        line,
        joiner_to_previous: LineJoiner::HardBreak,
        links: Vec::new(),
    }
}
