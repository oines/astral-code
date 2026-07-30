use ratatui::text::Text;

use super::RenderOptions;
use super::indented_lines;
use super::tool_header;
use super::truncate_head_tail;
use crate::DisplayMode;
use crate::ToolPresentation;

pub(super) fn render_execute(tool: &ToolPresentation, options: RenderOptions) -> Text<'static> {
    let label = if tool.is_user_shell() {
        "Run (user)"
    } else {
        "Run"
    };
    let mut lines = vec![tool_header(
        tool,
        label,
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
            DisplayMode::Truncated => truncate_head_tail(output_lines, options.max_output_lines),
            DisplayMode::Expanded => output_lines,
            DisplayMode::Collapsed => unreachable!("collapsed execute returned before output"),
        });
    }
    Text::from(lines)
}

fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
