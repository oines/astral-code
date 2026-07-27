//! Shared Astral modal chrome.
//!
//! The frame hierarchy is ported from Grok Build's `views/modal_window.rs` at
//! commit `47348d13ec4508dcfe440e34c6d511bb02998fb2` (Apache-2.0).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Widget;

use crate::modal::ModalState;

use super::AstralTheme;

pub(crate) struct InfoModal<'a> {
    pub(crate) state: &'a ModalState,
}

impl InfoModal<'_> {
    pub(crate) fn render(self, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
        let Some(popup) = popup_area(area) else {
            return;
        };
        Clear.render(popup, buffer);
        buffer.set_style(popup, Style::default().bg(theme.bg_base));
        let border = Style::default().fg(theme.gray_dim).bg(theme.bg_base);
        let title = Line::from(vec![
            "─ ".fg(theme.gray_dim),
            self.state.title.as_str().bold(),
            " ─".fg(theme.gray_dim),
        ]);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border)
            .style(Style::default().fg(theme.text_primary).bg(theme.bg_base))
            .title(title);
        let inner = block.inner(popup);
        block.render(popup, buffer);

        let close = "[×]";
        let close_x = popup.right().saturating_sub(5);
        buffer.set_string(
            close_x,
            popup.y,
            close,
            Style::default()
                .fg(theme.text_secondary)
                .bg(theme.bg_base)
                .add_modifier(Modifier::BOLD),
        );

        let content = Rect::new(
            inner.x + 2,
            inner.y + 1,
            inner.width.saturating_sub(4),
            inner.height.saturating_sub(3),
        );
        let label_width = self
            .state
            .rows
            .iter()
            .map(|row| Line::from(row.label.as_str()).width())
            .max()
            .unwrap_or_default();
        for (index, row) in self
            .state
            .rows
            .iter()
            .take(usize::from(content.height))
            .enumerate()
        {
            let y = content.y + u16::try_from(index).unwrap_or(u16::MAX);
            buffer.set_stringn(
                content.x,
                y,
                &row.label,
                usize::from(content.width),
                Style::default().fg(theme.gray).bg(theme.bg_base),
            );
            let value_x = content.x + u16::try_from(label_width).unwrap_or(u16::MAX) + 2;
            if value_x < content.right() {
                buffer.set_stringn(
                    value_x,
                    y,
                    &row.value,
                    usize::from(content.right().saturating_sub(value_x)),
                    Style::default().fg(theme.text_primary).bg(theme.bg_base),
                );
            }
        }

        let hint = "Esc close";
        let hint_width = u16::try_from(Line::from(hint).width()).unwrap_or(u16::MAX);
        if hint_width < inner.width {
            buffer.set_string(
                inner.x + (inner.width - hint_width) / 2,
                inner.bottom().saturating_sub(1),
                hint,
                Style::default().fg(theme.gray).bg(theme.bg_base),
            );
        }
    }
}

fn popup_area(area: Rect) -> Option<Rect> {
    if area.width < 20 || area.height < 8 {
        return None;
    }
    let width = (area.width.saturating_mul(3) / 5).clamp(44.min(area.width), 120.min(area.width));
    let height = area.height.saturating_sub(8).max(8).min(area.height);
    Some(Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    ))
}
