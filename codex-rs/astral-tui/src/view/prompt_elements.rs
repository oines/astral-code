//! Styling overlay for atomic prompt elements.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use std::ops::Range;

use super::AstralTheme;
use crate::composer::ComposerElement;

pub(super) struct PromptElementOverlay<'a> {
    pub(super) text: &'a str,
    pub(super) elements: &'a [ComposerElement],
    pub(super) rows: &'a [Range<usize>],
    pub(super) first_visible: usize,
    pub(super) visible_rows: usize,
}

impl PromptElementOverlay<'_> {
    pub(super) fn render(self, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
        let content_x = area.x.saturating_add(2);
        for element in self
            .elements
            .iter()
            .filter(|element| element.matches_text(self.text))
        {
            let foreground = if element.is_paste() {
                theme.text_secondary
            } else if element.is_file_reference() {
                theme.path
            } else {
                theme.accent_running
            };
            let style = Style::default().fg(foreground).bg(theme.panel_selected);
            for (visible_row, (row, range)) in self
                .rows
                .iter()
                .enumerate()
                .skip(self.first_visible)
                .take(self.visible_rows)
                .enumerate()
            {
                let start = element.range.start.max(range.start);
                let end = element.range.end.min(range.end);
                if start >= end {
                    continue;
                }
                let prefix = u16::from(row == 0) * 2;
                let offset = u16::try_from(Line::from(&self.text[range.start..start]).width())
                    .unwrap_or(u16::MAX);
                let width =
                    u16::try_from(Line::from(&self.text[start..end]).width()).unwrap_or(u16::MAX);
                let x = content_x.saturating_add(prefix).saturating_add(offset);
                let y = area
                    .y
                    .saturating_add(1)
                    .saturating_add(u16::try_from(visible_row).unwrap_or(u16::MAX));
                let end_x = x.saturating_add(width).min(area.right().saturating_sub(2));
                if x >= end_x {
                    continue;
                }
                for cell_x in x..end_x {
                    if let Some(cell) = buffer.cell_mut((cell_x, y)) {
                        cell.set_style(style);
                    }
                }
                if element.is_bracketed_chip() {
                    if start == element.range.start {
                        dim_bracket(buffer, x, y, theme);
                    }
                    if end == element.range.end {
                        dim_bracket(buffer, end_x.saturating_sub(1), y, theme);
                    }
                }
            }
        }
    }
}

fn dim_bracket(buffer: &mut Buffer, x: u16, y: u16, theme: AstralTheme) {
    if let Some(cell) = buffer.cell_mut((x, y)) {
        cell.set_style(Style::default().fg(theme.gray).bg(theme.panel_selected));
    }
}
