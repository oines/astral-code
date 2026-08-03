use codex_ansi_escape::ansi_escape;
use codex_app_server_protocol::CommandExecutionSource;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_app_server_protocol::ThreadItem;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use crate::DisplayMode;
use crate::EntryDisplayState;
use crate::LineJoiner;
use crate::LiveItem;
use crate::MarkdownLine;
use crate::highlight_fenced_code;
use crate::wrap_styled_line_with_metadata;

use super::super::EntryRenderOptions;
use super::super::prefix_lines;
use super::super::truncate_with_ellipsis;

pub(super) fn render(
    item: &ThreadItem,
    live: &LiveItem,
    state: EntryDisplayState,
    options: EntryRenderOptions,
) -> Option<Vec<MarkdownLine>> {
    let ThreadItem::CommandExecution {
        command,
        source,
        status,
        aggregated_output,
        exit_code,
        duration_ms,
        ..
    } = item
    else {
        return None;
    };

    let mut lines = render_header(
        command,
        *source,
        status,
        *exit_code,
        *duration_ms,
        state.mode(),
        options,
    );
    if state.mode() == DisplayMode::Collapsed {
        return Some(lines);
    }

    let terminal_input = live.terminal_input();
    if !terminal_input.is_empty() {
        lines.extend(render_terminal_input(terminal_input, options.width));
    }

    let output = aggregated_output
        .as_deref()
        .filter(|output| !output.is_empty())
        .or_else(|| live.command_output());
    if let Some(output) = output {
        lines.extend(render_output(output, state.mode(), options));
    }
    Some(lines)
}

fn render_header(
    command: &str,
    source: CommandExecutionSource,
    status: &CommandExecutionStatus,
    exit_code: Option<i32>,
    duration_ms: Option<i64>,
    mode: DisplayMode,
    options: EntryRenderOptions,
) -> Vec<MarkdownLine> {
    let label = if source == CommandExecutionSource::UserShell {
        "Run (user) "
    } else {
        "Run "
    };
    let prefix = Line::from(vec![status_marker(status), label.bold().dim()]);
    let prefix_width = u16::try_from(prefix.width()).unwrap_or(u16::MAX);
    let body_width = options.width.saturating_sub(prefix_width).max(1);
    let command = if mode == DisplayMode::Collapsed {
        command.split_whitespace().collect::<Vec<_>>().join(" ")
    } else {
        command.to_string()
    };
    let command = if command.trim().is_empty() {
        "…".to_string()
    } else {
        command
    };
    let highlighted = highlight_fenced_code(&command, "bash", options.markdown_style.syntax_theme)
        .unwrap_or_else(|| vec![vec![command.clone().dim()]]);

    let mut lines = Vec::new();
    for source_line in highlighted {
        let source_line = Line::from(source_line);
        let mut wrapped = wrap_styled_line_with_metadata(&source_line, body_width);
        if wrapped.is_empty() {
            wrapped.push(markdown_line(Line::default()));
        }
        lines.extend(wrapped);
    }
    prefix_lines(
        &mut lines,
        prefix,
        Line::from(" ".repeat(usize::from(prefix_width))),
    );

    if mode == DisplayMode::Collapsed {
        lines.truncate(1);
        if let Some(line) = lines.first_mut() {
            append_status_suffix(line, status, exit_code, duration_ms);
            truncate_with_ellipsis(line, options.width);
        }
        return lines;
    }

    if let Some(line) = lines.last_mut() {
        append_status_suffix(line, status, exit_code, duration_ms);
    }
    lines
}

fn render_terminal_input(input: &[String], width: u16) -> Vec<MarkdownLine> {
    let mut lines = input
        .iter()
        .flat_map(|input| {
            let line = Line::from(input.trim_end_matches(['\n', '\r']).to_string().dim());
            wrap_styled_line_with_metadata(&line, width.saturating_sub(4).max(1))
        })
        .collect::<Vec<_>>();
    prefix_lines(&mut lines, Line::from("  ↳ ".dim()), Line::from("    "));
    lines
}

fn render_output(
    output: &str,
    mode: DisplayMode,
    options: EntryRenderOptions,
) -> Vec<MarkdownLine> {
    let mut lines = ansi_escape(output)
        .lines
        .iter()
        .flat_map(|line| {
            wrap_styled_line_with_metadata(line, options.width.saturating_sub(4).max(1))
        })
        .collect::<Vec<_>>();
    if mode == DisplayMode::Truncated && lines.len() > options.max_truncated_lines {
        lines = truncate_head_tail(lines, options.max_truncated_lines);
    }
    prefix_lines(
        &mut lines,
        Line::from("  │ ".dim()),
        Line::from("  │ ".dim()),
    );
    lines
}

fn truncate_head_tail(mut lines: Vec<MarkdownLine>, max_lines: usize) -> Vec<MarkdownLine> {
    if lines.len() <= max_lines {
        return lines;
    }
    let first = max_lines.div_ceil(2);
    let last = max_lines.saturating_sub(first);
    let hidden = lines.len().saturating_sub(first + last);
    let tail = lines.split_off(lines.len().saturating_sub(last));
    lines.truncate(first);
    lines.push(markdown_line(Line::from(
        format!("… {hidden} hidden lines").dim(),
    )));
    lines.extend(tail);
    lines
}

fn append_status_suffix(
    line: &mut MarkdownLine,
    status: &CommandExecutionStatus,
    exit_code: Option<i32>,
    duration_ms: Option<i64>,
) {
    if *status == CommandExecutionStatus::Failed
        && let Some(exit_code) = exit_code
    {
        line.line.push_span(format!("  exit {exit_code}").red());
    } else if *status == CommandExecutionStatus::Declined {
        line.line.push_span("  declined".dim());
    }
    if let Some(duration_ms) = duration_ms {
        line.line
            .push_span(format!("  {}", duration_label(duration_ms)).dim());
    }
}

fn status_marker(status: &CommandExecutionStatus) -> Span<'static> {
    match status {
        CommandExecutionStatus::InProgress => "◇ ".magenta(),
        CommandExecutionStatus::Completed => "◆ ".green(),
        CommandExecutionStatus::Failed => "× ".red(),
        CommandExecutionStatus::Declined => "– ".dim(),
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
