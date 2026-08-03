//! Read card derived from Grok Build's `ReadToolCallBlock` at commit
//! `47348d13ec4508dcfe440e34c6d511bb02998fb2` (Apache-2.0).

use std::path::Path;

use codex_app_server_protocol::CoreToolCallStatus;
use codex_app_server_protocol::ThreadItem;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use crate::CodeLineHighlighter;
use crate::DisplayMode;
use crate::EntryDisplayState;
use crate::LineJoiner;
use crate::MarkdownLine;
use crate::read_tool::ReadCall;
use crate::render_literal_with_metadata;
use crate::wrap_styled_line_with_metadata;

use super::super::EntryRenderOptions;
use super::super::prefix_lines;
use super::super::truncate_with_ellipsis;

const HEAD_LINES: usize = 5;
const TAIL_LINES: usize = 3;
const GUTTER_GAP: &str = "  ";

pub(super) fn render(
    item: &ThreadItem,
    state: EntryDisplayState,
    options: EntryRenderOptions,
) -> Option<Vec<MarkdownLine>> {
    let call = ReadCall::from_item(item)?;
    let mut lines = render_header(call, state.mode(), options);
    if state.mode() == DisplayMode::Collapsed {
        return Some(lines);
    }
    let body = if let Some(error) = call.error() {
        render_notice(error, options.width, Style::default().red())
    } else if let Some(result) = call.result() {
        render_body(result, call.path(), state.mode(), options)
    } else {
        Vec::new()
    };
    if !body.is_empty() {
        lines.push(markdown_line(Line::default()));
        lines.extend(body);
    }
    Some(lines)
}

fn render_header(
    call: ReadCall<'_>,
    mode: DisplayMode,
    options: EntryRenderOptions,
) -> Vec<MarkdownLine> {
    let path = if mode == DisplayMode::Collapsed {
        compact_path(call.path())
    } else {
        call.path().to_string()
    };
    let mut body = Line::from(Span::styled(
        path,
        Style::default().fg(options.diff_style.path),
    ));
    if let Some(suffix) = range_suffix(call) {
        body.push_span(format!(" {suffix}").dim());
    }
    if let Some(duration_ms) = call.duration_ms().filter(|duration| *duration >= 0) {
        body.push_span(format!("  {}", duration_label(duration_ms)).dim());
    }
    let prefix = Line::from(vec![status_marker(call.status()), "Read ".bold().dim()]);
    let prefix_width = u16::try_from(prefix.width()).unwrap_or(u16::MAX);
    let mut lines =
        wrap_styled_line_with_metadata(&body, options.width.saturating_sub(prefix_width).max(1));
    prefix_lines(
        &mut lines,
        prefix,
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

fn render_body(
    result: &str,
    path: &str,
    mode: DisplayMode,
    options: EntryRenderOptions,
) -> Vec<MarkdownLine> {
    let Some(rows) = numbered_rows(result) else {
        return render_notice(
            clean_system_reminder(result),
            options.width,
            Style::default().dim(),
        );
    };
    let number_width = rows
        .iter()
        .map(|row| row.number)
        .max()
        .unwrap_or(1)
        .to_string()
        .len();
    let prefix_width = 2 + number_width + Line::from(GUTTER_GAP).width();
    let content_width = usize::from(options.width)
        .saturating_sub(prefix_width)
        .max(1);
    let source = rows
        .iter()
        .map(|row| row.content)
        .collect::<Vec<_>>()
        .join("\n");
    let mut highlighter = CodeLineHighlighter::for_path(
        Path::new(path),
        &source,
        options.markdown_style.syntax_theme,
    );
    let mut lines = Vec::new();
    for row in rows {
        let spans = highlighter
            .as_mut()
            .and_then(|highlighter| highlighter.highlight_line(row.content))
            .unwrap_or_else(|| vec![row.content.to_string().into()]);
        let mut wrapped = wrap_styled_line_with_metadata(
            &Line::from(spans),
            u16::try_from(content_width).unwrap_or(u16::MAX),
        );
        for (index, line) in wrapped.iter_mut().enumerate() {
            let number = if index == 0 {
                format!("{:>number_width$}", row.number).dim()
            } else {
                " ".repeat(number_width).into()
            };
            line.line
                .spans
                .splice(0..0, ["  ".into(), number, GUTTER_GAP.into()]);
        }
        lines.extend(wrapped);
    }
    if mode == DisplayMode::Truncated {
        truncate(lines)
    } else {
        lines
    }
}

fn range_suffix(call: ReadCall<'_>) -> Option<String> {
    let result = call.result()?;
    if call.empty() {
        return Some("(empty)".to_string());
    }
    if call.unchanged() {
        return Some("(unchanged)".to_string());
    }
    if call.offset().is_some() || call.limit().is_some() {
        let rows = numbered_rows(result)?;
        return Some(format!(
            "({}–{})",
            rows.first()?.number,
            rows.last()?.number
        ));
    }
    None
}

fn numbered_rows(result: &str) -> Option<Vec<ReadRow<'_>>> {
    let mut rows = Vec::new();
    for line in result.lines() {
        let (number, content) = line.split_once('\t')?;
        rows.push(ReadRow {
            number: number.parse().ok()?,
            content,
        });
    }
    (!rows.is_empty()).then_some(rows)
}

fn truncate(mut lines: Vec<MarkdownLine>) -> Vec<MarkdownLine> {
    let visible = HEAD_LINES + TAIL_LINES;
    if lines.len() <= visible {
        return lines;
    }
    let hidden = lines.len().saturating_sub(visible);
    let tail = lines.split_off(lines.len().saturating_sub(TAIL_LINES));
    lines.truncate(HEAD_LINES);
    lines.push(markdown_line(Line::from(
        format!("  … {hidden} hidden lines").dim(),
    )));
    lines.extend(tail);
    lines
}

fn render_notice(text: &str, width: u16, style: Style) -> Vec<MarkdownLine> {
    let mut lines = render_literal_with_metadata(text, width.saturating_sub(4).max(1), style);
    prefix_lines(
        &mut lines,
        Line::from("  │ ".dim()),
        Line::from("  │ ".dim()),
    );
    lines
}

fn clean_system_reminder(result: &str) -> &str {
    result
        .strip_prefix("<system-reminder>")
        .and_then(|result| result.strip_suffix("</system-reminder>"))
        .unwrap_or(result)
}

fn compact_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map_or_else(|| path.to_string(), str::to_string)
}

fn status_marker(status: CoreToolCallStatus) -> Span<'static> {
    match status {
        CoreToolCallStatus::InProgress => "◇ ".magenta(),
        CoreToolCallStatus::Completed => "◆ ".green(),
        CoreToolCallStatus::Failed => "× ".red(),
        CoreToolCallStatus::Interrupted => "– ".dim(),
    }
}

fn duration_label(duration_ms: i64) -> String {
    if duration_ms < 1_000 {
        format!("{}ms", duration_ms.max(0))
    } else {
        format!("{:.1}s", duration_ms.max(0) as f64 / 1_000.0)
    }
}

fn markdown_line(line: Line<'static>) -> MarkdownLine {
    MarkdownLine {
        line,
        joiner_to_previous: LineJoiner::HardBreak,
        links: Vec::new(),
    }
}

struct ReadRow<'a> {
    number: usize,
    content: &'a str,
}
