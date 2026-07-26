use ratatui::style::Color;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use textwrap::Options;

use crate::PresentationBlock;
use crate::ToolKind;
use crate::ToolPresentation;
use crate::ToolStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderOptions {
    pub width: u16,
    pub expanded: bool,
    pub max_output_lines: usize,
}

impl RenderOptions {
    pub fn compact(width: u16) -> Self {
        Self {
            width,
            expanded: false,
            max_output_lines: 3,
        }
    }
}

pub fn render_block(block: &PresentationBlock, options: RenderOptions) -> Text<'static> {
    match block {
        PresentationBlock::User { text, attachments } => {
            render_user(text, attachments, options.width)
        }
        PresentationBlock::Assistant { text } => render_assistant(text, options.width),
        PresentationBlock::Thinking { text, running } => render_thinking(text, *running, options),
        PresentationBlock::Plan { text, running } => render_plan(text, *running, options),
        PresentationBlock::Tool(tool) => render_tool(tool, options),
        PresentationBlock::System { title, detail } => {
            let mut lines = vec![vec!["◆ ".dim(), title.clone().dim()].into()];
            if options.expanded
                && let Some(detail) = detail
            {
                lines.extend(indented_lines(detail, options.width, "  ", true));
            }
            Text::from(lines)
        }
    }
}

fn render_user(text: &str, attachments: &[String], width: u16) -> Text<'static> {
    let mut lines = prefixed_lines(text, width, "❯ ", "  ", false);
    lines.extend(
        attachments
            .iter()
            .flat_map(|attachment| prefixed_lines(attachment, width, "  ↳ ", "    ", true)),
    );
    Text::from(lines)
}

fn render_assistant(text: &str, width: u16) -> Text<'static> {
    Text::from(wrapped_lines(text, width, "", "", false))
}

fn render_thinking(text: &str, running: bool, options: RenderOptions) -> Text<'static> {
    let marker = if running { "◇ " } else { "◆ " };
    let mut lines = vec![vec![marker.magenta(), "Thinking".dim().italic()].into()];
    if options.expanded || running {
        lines.extend(indented_lines(text, options.width, "  ", true));
    }
    Text::from(lines)
}

fn render_plan(text: &str, running: bool, options: RenderOptions) -> Text<'static> {
    let marker = if running { "◇ " } else { "◆ " };
    let mut lines = vec![vec![marker.cyan(), "Plan".cyan()].into()];
    if options.expanded || running {
        lines.extend(indented_lines(text, options.width, "  ", false));
    }
    Text::from(lines)
}

fn render_tool(tool: &ToolPresentation, options: RenderOptions) -> Text<'static> {
    let marker = status_marker(tool.status);
    let mut header = vec![
        marker.set_style(status_style(tool.status)),
        format!("{} ", tool_verb(tool.kind, tool.status)).into(),
        tool.title.clone().bold(),
    ];
    if let Some(duration_ms) = tool.duration_ms {
        header.push(format!("  {}", duration_label(duration_ms)).dim());
    }
    let mut lines = vec![Line::from(header)];

    if options.expanded {
        lines.extend(
            tool.details
                .iter()
                .flat_map(|detail| indented_lines(detail, options.width, "  ", true)),
        );
        for change in &tool.changes {
            let (added, removed) = diff_counts(&change.diff);
            lines.push(
                vec![
                    "  ".into(),
                    change.path.clone().dim(),
                    format!("  +{added}").green(),
                    format!(" -{removed}").red(),
                ]
                .into(),
            );
        }
    }

    if let Some(output) = tool.output.as_deref() {
        let mut output_lines = indented_lines(output, options.width, "  │ ", true);
        let total = output_lines.len();
        if !options.expanded && total > options.max_output_lines {
            output_lines.truncate(options.max_output_lines);
            output_lines.push(
                vec![
                    "  └ ".dim(),
                    format!("{} more lines", total - options.max_output_lines).dim(),
                ]
                .into(),
            );
        }
        lines.extend(output_lines);
    }
    Text::from(lines)
}

fn wrapped_lines(
    value: &str,
    width: u16,
    initial_indent: &str,
    subsequent_indent: &str,
    dim: bool,
) -> Vec<Line<'static>> {
    let width = usize::from(width).max(1);
    let options = Options::new(width)
        .initial_indent(initial_indent)
        .subsequent_indent(subsequent_indent)
        .break_words(false);
    value
        .split('\n')
        .flat_map(|line| {
            if line.is_empty() {
                vec![Line::default()]
            } else {
                textwrap::wrap(line, &options)
                    .into_iter()
                    .map(|line| styled_line(line.into_owned(), dim))
                    .collect()
            }
        })
        .collect()
}

fn prefixed_lines(
    value: &str,
    width: u16,
    initial_indent: &str,
    subsequent_indent: &str,
    dim: bool,
) -> Vec<Line<'static>> {
    wrapped_lines(value, width, initial_indent, subsequent_indent, dim)
}

fn indented_lines(value: &str, width: u16, indent: &str, dim: bool) -> Vec<Line<'static>> {
    wrapped_lines(value, width, indent, indent, dim)
}

fn styled_line(value: String, dim: bool) -> Line<'static> {
    if dim {
        Line::from(value.dim())
    } else {
        Line::from(value)
    }
}

fn status_marker(status: ToolStatus) -> Span<'static> {
    match status {
        ToolStatus::Running => "◇ ".into(),
        ToolStatus::Success => "◆ ".into(),
        ToolStatus::Failed => "× ".into(),
        ToolStatus::Declined => "– ".into(),
        ToolStatus::Interrupted => "■ ".into(),
    }
}

fn status_style(status: ToolStatus) -> ratatui::style::Style {
    match status {
        ToolStatus::Running => ratatui::style::Style::default().fg(Color::Magenta),
        ToolStatus::Success => ratatui::style::Style::default().fg(Color::Green),
        ToolStatus::Failed => ratatui::style::Style::default().fg(Color::Red),
        ToolStatus::Declined => ratatui::style::Style::default().fg(Color::Yellow),
        ToolStatus::Interrupted => ratatui::style::Style::default().fg(Color::DarkGray),
    }
}

fn tool_verb(kind: ToolKind, status: ToolStatus) -> &'static str {
    let running = status == ToolStatus::Running;
    match (kind, running) {
        (ToolKind::Execute, true) => "Running",
        (ToolKind::Execute, false) => "Ran",
        (ToolKind::Read, true) => "Reading",
        (ToolKind::Read, false) => "Read",
        (ToolKind::Edit, true) => "Editing",
        (ToolKind::Edit, false) => "Edited",
        (ToolKind::List, true) => "Listing",
        (ToolKind::List, false) => "Listed",
        (ToolKind::Search | ToolKind::WebSearch, true) => "Searching",
        (ToolKind::Search | ToolKind::WebSearch, false) => "Searched",
        (ToolKind::WebFetch, true) => "Fetching",
        (ToolKind::WebFetch, false) => "Fetched",
        (ToolKind::Mcp, true) => "Calling",
        (ToolKind::Mcp, false) => "Called",
        (ToolKind::Skill, true) => "Loading",
        (ToolKind::Skill, false) => "Loaded",
        (ToolKind::Collab, true) => "Coordinating",
        (ToolKind::Collab, false) => "Coordinated",
        (ToolKind::Media, true) => "Creating",
        (ToolKind::Media, false) => "Created",
        (ToolKind::Other, true) => "Using",
        (ToolKind::Other, false) => "Used",
    }
}

fn duration_label(duration_ms: i64) -> String {
    if duration_ms < 1_000 {
        format!("{duration_ms}ms")
    } else {
        format!("{:.1}s", duration_ms as f64 / 1_000.0)
    }
}

fn diff_counts(diff: &str) -> (usize, usize) {
    diff.lines()
        .filter(|line| !line.starts_with("+++") && !line.starts_with("---"))
        .fold((0, 0), |(added, removed), line| {
            (
                added + usize::from(line.starts_with('+')),
                removed + usize::from(line.starts_with('-')),
            )
        })
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
