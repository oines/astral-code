use ratatui::style::Stylize;
use ratatui::text::Line;

use crate::MarkdownLine;
use crate::VerbGroupSpan;
use crate::wrap_styled_line_with_metadata;

use super::prefix_lines;

pub(super) fn render_header(group: &VerbGroupSpan, width: u16) -> Vec<MarkdownLine> {
    let marker = if group.failed() {
        "× ".red()
    } else if group.running() {
        "◇ ".magenta()
    } else {
        "◆ ".dim()
    };
    let mut label = Line::from(group.label().to_string().bold().dim());
    if group.failed()
        && let Some((summary, failed)) = group.label().rsplit_once(" · ")
    {
        label = Line::from(vec![
            summary.to_string().bold().dim(),
            " · ".dim(),
            failed.to_string().red(),
        ]);
    }
    let prefix_width = u16::try_from(Line::from(marker.clone()).width()).unwrap_or(u16::MAX);
    let mut lines =
        wrap_styled_line_with_metadata(&label, width.saturating_sub(prefix_width).max(1));
    prefix_lines(
        &mut lines,
        Line::from(marker),
        Line::from(" ".repeat(usize::from(prefix_width))),
    );
    lines
}
