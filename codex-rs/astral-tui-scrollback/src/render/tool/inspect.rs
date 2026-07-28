use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use textwrap::Options;

use super::RenderOptions;
use super::indented_lines;
use super::tool_header;
use super::truncate_head_tail;
use crate::DisplayMode;
use crate::ToolKind;
use crate::ToolPresentation;
use crate::ToolStatus;

pub(super) fn render_inspection(tool: &ToolPresentation, options: RenderOptions) -> Text<'static> {
    let mut lines = vec![tool_header(
        tool,
        inspection_label(tool.kind),
        &tool.title,
        collapsed_summary(tool, options.mode),
    )];
    if options.mode == DisplayMode::Collapsed {
        return Text::from(lines);
    }

    if let Some(output) = tool
        .output
        .as_deref()
        .filter(|output| !output.trim().is_empty())
    {
        let body = match tool.kind {
            ToolKind::Read if !has_line_number_gutter(output) => {
                numbered_lines(output, options.width)
            }
            ToolKind::Read | ToolKind::List | ToolKind::Search => {
                indented_lines(output, options.width, "  ", true)
            }
            _ => unreachable!("inspection renderer received a non-inspection tool"),
        };
        lines.extend(match options.mode {
            DisplayMode::Truncated => truncate_head_tail(body, options.max_output_lines),
            DisplayMode::Expanded => body,
            DisplayMode::Collapsed => unreachable!("collapsed inspection returned before output"),
        });
    }
    Text::from(lines)
}

fn inspection_label(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Read => "Read",
        ToolKind::List => "List",
        ToolKind::Search => "Search",
        _ => unreachable!("inspection label requires a read, list, or search tool"),
    }
}

fn collapsed_summary(tool: &ToolPresentation, mode: DisplayMode) -> Vec<Span<'static>> {
    if mode != DisplayMode::Collapsed || tool.status != ToolStatus::Success {
        return Vec::new();
    }
    let Some(output) = tool
        .output
        .as_deref()
        .filter(|output| !output.trim().is_empty())
    else {
        return Vec::new();
    };
    let count = match tool.kind {
        ToolKind::Read => output.lines().count(),
        ToolKind::List | ToolKind::Search => output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count(),
        _ => unreachable!("inspection summary requires a read, list, or search tool"),
    };
    let unit = match (tool.kind, count) {
        (ToolKind::List, 1) => "entry",
        (ToolKind::List, _) => "entries",
        (ToolKind::Read, 1) => "line",
        (ToolKind::Read, _) => "lines",
        (ToolKind::Search, 1) => "result",
        (ToolKind::Search, _) => "results",
        _ => unreachable!("inspection summary requires a read, list, or search tool"),
    };
    vec![format!(" ({count} {unit})").dim()]
}

fn numbered_lines(value: &str, width: u16) -> Vec<Line<'static>> {
    let line_count = value.lines().count();
    let gutter_width = decimal_width(line_count);
    value
        .lines()
        .enumerate()
        .flat_map(|(index, line)| {
            let line_number = index + 1;
            let prefix = format!("  {line_number:>gutter_width$}  ");
            let continuation = " ".repeat(prefix.chars().count());
            let options = Options::new(usize::from(width).max(1))
                .initial_indent(&prefix)
                .subsequent_indent(&continuation)
                .word_separator(textwrap::WordSeparator::AsciiSpace)
                .word_splitter(textwrap::WordSplitter::NoHyphenation)
                .break_words(true);
            if line.is_empty() {
                vec![Line::from(prefix.trim_end().to_string().dim())]
            } else {
                textwrap::wrap(line, &options)
                    .into_iter()
                    .map(|line| Line::from(line.into_owned().dim()))
                    .collect::<Vec<_>>()
            }
        })
        .collect()
}

fn has_line_number_gutter(value: &str) -> bool {
    value
        .lines()
        .filter(|line| !line.trim().is_empty())
        .any(|line| {
            let trimmed = line.trim_start();
            let digit_count = trimmed.chars().take_while(char::is_ascii_digit).count();
            digit_count > 0
                && matches!(
                    trimmed.chars().nth(digit_count),
                    Some('→' | '|' | ':' | '\t')
                )
        })
}

fn decimal_width(value: usize) -> usize {
    value
        .checked_ilog10()
        .map_or(1, |digits| digits as usize + 1)
}
