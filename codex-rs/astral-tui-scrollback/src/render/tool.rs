use ratatui::style::Style;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;

use super::RenderOptions;
use super::indented_lines;
use super::status_marker;
use super::status_style;
use crate::DisplayMode;
use crate::ToolKind;
use crate::ToolPresentation;

mod edit;
mod execute;
mod inspect;

pub(super) fn render_edit_viewer_lines(
    tool: &ToolPresentation,
    options: RenderOptions,
) -> Vec<super::EditViewerLine> {
    edit::render_edit_viewer_lines(tool, options)
}

pub(super) fn render_tool(tool: &ToolPresentation, options: RenderOptions) -> Text<'static> {
    match tool.kind {
        ToolKind::Edit if !tool.changes.is_empty() => edit::render_edit(tool, options),
        ToolKind::Execute | ToolKind::Background => execute::render_execute(tool, options),
        ToolKind::Read | ToolKind::List | ToolKind::Search => {
            inspect::render_inspection(tool, options)
        }
        ToolKind::BackgroundPoll
        | ToolKind::BackgroundInput
        | ToolKind::BackgroundList
        | ToolKind::BackgroundStop
        | ToolKind::Edit
        | ToolKind::WebFetch
        | ToolKind::WebSearch
        | ToolKind::Mcp
        | ToolKind::Skill
        | ToolKind::Collab
        | ToolKind::ImageView
        | ToolKind::ImageGeneration
        | ToolKind::Todo
        | ToolKind::Other => render_generic_tool(tool, options),
    }
}

fn render_generic_tool(tool: &ToolPresentation, options: RenderOptions) -> Text<'static> {
    let marker = status_marker(tool.status);
    let mut header = vec![marker.set_style(status_style(tool.status))];
    if tool.kind == ToolKind::Mcp {
        header.extend(mcp_header(tool));
    } else {
        header.extend([
            format!("{} ", tool_label(tool.kind)).bold().dim(),
            tool.title.clone().dim(),
        ]);
    }
    if let Some(duration_ms) = tool.duration_ms {
        header.push(format!("  {}", duration_label(duration_ms)).dim());
    }
    let mut lines = vec![Line::from(header)];

    if options.mode == DisplayMode::Expanded {
        lines.extend(
            tool.details
                .iter()
                .flat_map(|detail| indented_lines(detail, options.width, "  ", true)),
        );
    }

    if options.mode != DisplayMode::Collapsed
        && let Some(output) = tool.output.as_deref()
    {
        let mut output_lines = indented_lines(output, options.width, "  │ ", true);
        let total = output_lines.len();
        if options.mode == DisplayMode::Truncated && total > options.max_output_lines {
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

fn tool_header(
    tool: &ToolPresentation,
    label: &str,
    title: &str,
    suffix: Vec<Span<'static>>,
) -> Line<'static> {
    tool_header_with_title_style(tool, label, title, Style::default().dim(), suffix)
}

fn tool_header_with_title_style(
    tool: &ToolPresentation,
    label: &str,
    title: &str,
    title_style: ratatui::style::Style,
    suffix: Vec<Span<'static>>,
) -> Line<'static> {
    let mut spans = vec![
        status_marker(tool.status).set_style(status_style(tool.status)),
        format!("{label} ").bold().dim(),
        Span::styled(title.to_string(), title_style),
    ];
    spans.extend(suffix);
    if let Some(duration_ms) = tool.duration_ms {
        spans.push(format!("  {}", duration_label(duration_ms)).dim());
    }
    spans.into()
}

fn truncate_head_tail(lines: Vec<Line<'static>>, max_lines: usize) -> Vec<Line<'static>> {
    if lines.len() <= max_lines {
        return lines;
    }
    let first = max_lines.div_ceil(2);
    let last = max_lines.saturating_sub(first);
    let hidden = lines.len().saturating_sub(first + last);
    let mut visible = lines.iter().take(first).cloned().collect::<Vec<_>>();
    visible.push(vec!["  … ".dim(), format!("{hidden} hidden lines").dim()].into());
    if last > 0 {
        visible.extend(
            lines
                .into_iter()
                .rev()
                .take(last)
                .collect::<Vec<_>>()
                .into_iter()
                .rev(),
        );
    }
    visible
}

fn mcp_header(tool: &ToolPresentation) -> Vec<Span<'static>> {
    let (server, action) = tool
        .name
        .split_once('/')
        .unwrap_or(("", tool.name.as_str()));
    let mut spans = Vec::new();
    if !server.is_empty() {
        spans.push(format!("{server} ").bold().dim());
    }
    spans.push(action.replace(['_', '-'], " ").dim());
    if tool.title != action {
        spans.push(" · ".dim());
        spans.push(tool.title.clone().dim());
    }
    spans
}

fn tool_label(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Execute | ToolKind::Background => "Run",
        ToolKind::BackgroundPoll => "Poll",
        ToolKind::BackgroundInput => "Input",
        ToolKind::BackgroundList => "List",
        ToolKind::BackgroundStop => "Stop",
        ToolKind::Read => "Read",
        ToolKind::Edit => "Edit",
        ToolKind::List => "List",
        ToolKind::Search | ToolKind::WebSearch => "Search",
        ToolKind::WebFetch => "Fetch",
        ToolKind::Mcp => "",
        ToolKind::Skill => "Skill",
        ToolKind::Collab => "Subagent",
        ToolKind::ImageView => "View",
        ToolKind::ImageGeneration => "Generate",
        ToolKind::Todo => "Todo",
        ToolKind::Other => "Use",
    }
}

fn duration_label(duration_ms: i64) -> String {
    if duration_ms < 1_000 {
        format!("{duration_ms}ms")
    } else {
        format!("{:.1}s", duration_ms as f64 / 1_000.0)
    }
}
