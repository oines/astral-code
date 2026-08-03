//! Web-search and page-navigation cards derived from Grok Build's web tool
//! blocks at commit `47348d13ec4508dcfe440e34c6d511bb02998fb2` (Apache-2.0).

use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use crate::DisplayMode;
use crate::EntryDisplayState;
use crate::MarkdownLine;
use crate::WebSearchBlock;
use crate::wrap_styled_line_with_metadata;

use super::EntryRenderOptions;
use super::format_elapsed;
use super::prefix_lines;
use super::truncate_with_ellipsis;

pub(super) fn render(
    search: WebSearchBlock<'_>,
    state: EntryDisplayState,
    options: EntryRenderOptions,
) -> Vec<MarkdownLine> {
    let marker = if search.running() {
        "◇ ".magenta()
    } else {
        "◆ ".dim()
    };
    let prefix = Line::from(vec![marker, format!("{} ", search.label()).bold().dim()]);
    let prefix_width = u16::try_from(prefix.width()).unwrap_or(u16::MAX);
    let mut detail = Line::from(Span::styled(
        search.detail().into_owned(),
        options.markdown_style.inline_code,
    ));
    if !search.running()
        && let Some(elapsed_ms) = search.elapsed_ms()
    {
        detail.push_span(format!("  {}", format_elapsed(elapsed_ms)).dim());
    }
    let mut lines =
        wrap_styled_line_with_metadata(&detail, options.width.saturating_sub(prefix_width).max(1));
    prefix_lines(
        &mut lines,
        prefix,
        Line::from(" ".repeat(usize::from(prefix_width))),
    );
    if state.mode() == DisplayMode::Collapsed {
        let wrapped = lines.len() > 1;
        lines.truncate(1);
        if wrapped && let Some(line) = lines.first_mut() {
            truncate_with_ellipsis(line, options.width);
        }
    }
    lines
}
