use codex_app_server_protocol::PatchChangeKind;
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

pub(super) fn render_tool(tool: &ToolPresentation, options: RenderOptions) -> Text<'static> {
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
        for change in &tool.changes {
            let (added, removed) = diff_counts(&change.diff);
            let (operation, path) = match &change.kind {
                PatchChangeKind::Add => ("A".green(), change.path.clone()),
                PatchChangeKind::Delete => ("D".red(), change.path.clone()),
                PatchChangeKind::Update {
                    move_path: Some(move_path),
                } => (
                    "R".magenta(),
                    format!("{} → {}", change.path, move_path.display()),
                ),
                PatchChangeKind::Update { move_path: None } => ("M".cyan(), change.path.clone()),
            };
            lines.push(
                vec![
                    "  ".into(),
                    operation,
                    " ".dim(),
                    path.dim(),
                    format!("  +{added}").green(),
                    format!(" -{removed}").red(),
                ]
                .into(),
            );
        }
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
