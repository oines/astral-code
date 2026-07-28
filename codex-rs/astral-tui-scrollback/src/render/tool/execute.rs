use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Text;

use super::RenderOptions;
use super::indented_lines;
use super::tool_header;
use crate::DisplayMode;
use crate::ToolPresentation;

pub(super) fn render_execute(tool: &ToolPresentation, options: RenderOptions) -> Text<'static> {
    let mut lines = vec![tool_header(
        tool,
        "Run",
        &single_line(&tool.title),
        Vec::new(),
    )];
    if options.mode == DisplayMode::Collapsed {
        return Text::from(lines);
    }

    lines.extend(
        tool.details
            .iter()
            .flat_map(|detail| indented_lines(detail, options.width, "  ", true)),
    );
    if let Some(output) = tool
        .output
        .as_deref()
        .filter(|output| !output.trim().is_empty())
    {
        let output_lines = indented_lines(output, options.width, "  │ ", true);
        lines.extend(match options.mode {
            DisplayMode::Truncated => truncate_output(output_lines, options.max_output_lines),
            DisplayMode::Expanded => output_lines,
            DisplayMode::Collapsed => unreachable!("collapsed execute returned before output"),
        });
    }
    Text::from(lines)
}

fn truncate_output(lines: Vec<Line<'static>>, max_output_lines: usize) -> Vec<Line<'static>> {
    if lines.len() <= max_output_lines {
        return lines;
    }
    let first = max_output_lines.div_ceil(2);
    let last = max_output_lines.saturating_sub(first);
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

fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
