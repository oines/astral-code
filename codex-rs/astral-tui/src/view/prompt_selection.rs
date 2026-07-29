//! Selection overlay for the wrapped prompt text.

use std::ops::Range;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;

use super::AstralTheme;

pub(super) struct PromptSelectionOverlay<'a> {
    pub(super) text: &'a str,
    pub(super) selection: Option<Range<usize>>,
    pub(super) rows: &'a [Range<usize>],
    pub(super) first_visible: usize,
    pub(super) visible_rows: usize,
}

impl PromptSelectionOverlay<'_> {
    pub(super) fn render(self, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
        let Some(selection) = self.selection else {
            return;
        };
        let style = Style::default()
            .fg(theme.prompt_selection_foreground)
            .bg(theme.prompt_selection_background);
        let content_x = area.x.saturating_add(2);
        for (visible_row, (row, range)) in self
            .rows
            .iter()
            .enumerate()
            .skip(self.first_visible)
            .take(self.visible_rows)
            .enumerate()
        {
            let start = selection.start.max(range.start);
            let end = selection.end.min(range.end);
            if start >= end {
                continue;
            }
            let prefix = u16::from(row == 0) * 2;
            let selection_x = u16::try_from(Line::from(&self.text[range.start..start]).width())
                .unwrap_or(u16::MAX);
            let selection_width =
                u16::try_from(Line::from(&self.text[start..end]).width()).unwrap_or(u16::MAX);
            let x = content_x.saturating_add(prefix).saturating_add(selection_x);
            let y = area
                .y
                .saturating_add(1)
                .saturating_add(u16::try_from(visible_row).unwrap_or(u16::MAX));
            let end_x = x
                .saturating_add(selection_width)
                .min(area.right().saturating_sub(2));
            for cell_x in x..end_x {
                if let Some(cell) = buffer.cell_mut((cell_x, y)) {
                    cell.set_style(style);
                }
            }
        }
    }
}
