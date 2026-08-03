//! Shared presentation primitives for structured external tool cards.

use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use serde_json::Value;

use crate::DisplayMode;
use crate::LineJoiner;
use crate::MarkdownLine;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ToolCardStatus {
    Running,
    Succeeded,
    Failed,
}

pub(super) struct ToolCardHeader {
    pub title: Option<String>,
    pub detail: String,
    pub status: ToolCardStatus,
    pub duration_ms: Option<i64>,
}

pub(super) fn render_header(
    header: ToolCardHeader,
    mode: DisplayMode,
    options: EntryRenderOptions,
) -> Vec<MarkdownLine> {
    let marker = match header.status {
        ToolCardStatus::Running => "◇ ".magenta(),
        ToolCardStatus::Succeeded => "◆ ".green(),
        ToolCardStatus::Failed => "× ".red(),
    };
    let mut title = Line::default();
    if let Some(leading) = header.title.filter(|title| !title.is_empty()) {
        title.push_span(leading.bold().dim());
        if !header.detail.is_empty() {
            title.push_span(" ");
        }
    }
    if !header.detail.is_empty() {
        let detail = Span::styled(header.detail, Style::default().fg(options.diff_style.path));
        title.push_span(detail);
    }
    if header.status != ToolCardStatus::Running
        && let Some(duration_ms) = header.duration_ms.filter(|duration| *duration >= 0)
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

pub(super) fn render_arguments(
    arguments: &Value,
    options: EntryRenderOptions,
) -> Vec<MarkdownLine> {
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

pub(super) fn render_body(
    mut source_lines: Vec<String>,
    status: ToolCardStatus,
    options: EntryRenderOptions,
) -> Vec<MarkdownLine> {
    if source_lines.is_empty() {
        return Vec::new();
    }
    let hidden = source_lines.len().saturating_sub(MAX_OUTPUT_LINES);
    source_lines.truncate(MAX_OUTPUT_LINES);
    if hidden > 0 {
        source_lines.push(format!("… {hidden} hidden lines"));
    }
    let source = truncate_chars(&source_lines.join("\n"), MAX_OUTPUT_CHARS);
    let style = if status == ToolCardStatus::Failed {
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

pub(super) fn bounded_value(value: &Value, max_chars: usize) -> String {
    let value = value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_string);
    truncate_chars(&value, max_chars)
}

pub(super) fn bounded_output_lines(value: &str) -> Vec<String> {
    truncate_chars(value, MAX_OUTPUT_CHARS)
        .split('\n')
        .map(str::to_string)
        .collect()
}

pub(super) fn bounded_output_value(value: &Value) -> String {
    bounded_value(value, MAX_OUTPUT_CHARS)
}

pub(super) fn append_section(lines: &mut Vec<MarkdownLine>, section: Vec<MarkdownLine>) {
    if !section.is_empty() {
        lines.push(markdown_line(Line::default()));
        lines.extend(section);
    }
}

pub(super) fn titleize(name: &str) -> String {
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

fn markdown_line(line: Line<'static>) -> MarkdownLine {
    MarkdownLine {
        line,
        joiner_to_previous: LineJoiner::HardBreak,
        links: Vec::new(),
    }
}

#[cfg(test)]
#[path = "tool_card_tests.rs"]
mod tests;
