use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;

use crate::slash::SlashSnapshot;

use super::AstralTheme;

pub(crate) struct SlashMenu<'a> {
    pub(crate) snapshot: &'a SlashSnapshot,
}

impl SlashMenu<'_> {
    pub(crate) fn render(self, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
        if area.width < 12 || area.height < 3 {
            return;
        }
        let border = theme.prompt_border_active;
        buffer.set_style(
            area,
            Style::default().fg(theme.text_primary).bg(theme.bg_base),
        );
        for x in area.x..area.right() {
            set(
                buffer,
                x,
                area.y,
                if x == area.x {
                    '╭'
                } else if x == area.right() - 1 {
                    '╮'
                } else {
                    '─'
                },
                Style::default().fg(border).bg(theme.bg_base),
            );
            set(
                buffer,
                x,
                area.bottom() - 1,
                if x == area.x {
                    '╰'
                } else if x == area.right() - 1 {
                    '╯'
                } else {
                    '─'
                },
                Style::default().fg(border).bg(theme.bg_base),
            );
        }
        buffer.set_string(
            area.x + 2,
            area.y,
            " commands ",
            Style::default().fg(theme.gray).bg(theme.bg_base),
        );

        let rows = usize::from(area.height.saturating_sub(2));
        let start = self
            .snapshot
            .selected
            .saturating_add(1)
            .saturating_sub(rows);
        for (visible, (index, suggestion)) in self
            .snapshot
            .matches
            .iter()
            .enumerate()
            .skip(start)
            .take(rows)
            .enumerate()
        {
            let y = area.y + 1 + u16::try_from(visible).unwrap_or(u16::MAX);
            set(buffer, area.x, y, '│', Style::default().fg(border));
            set(
                buffer,
                area.right() - 1,
                y,
                '│',
                Style::default().fg(border),
            );
            let selected = index == self.snapshot.selected;
            let row_style = if selected {
                Style::default().fg(theme.bg_base).bg(theme.text_secondary)
            } else {
                Style::default().fg(theme.text_primary).bg(theme.bg_base)
            };
            let row = Rect::new(area.x + 1, y, area.width.saturating_sub(2), 1);
            buffer.set_style(row, row_style);
            let marker = if selected { "› " } else { "  " };
            buffer.set_string(row.x, y, marker, row_style);
            buffer.set_string(row.x + 2, y, &suggestion.display, row_style);
            if !selected {
                for index in &suggestion.indices {
                    let x = row.x + 3 + u16::try_from(*index).unwrap_or(u16::MAX);
                    if let Some(cell) = buffer.cell_mut((x, y)) {
                        cell.set_style(
                            Style::default()
                                .fg(theme.accent_running)
                                .bg(theme.bg_base)
                                .add_modifier(Modifier::BOLD),
                        );
                    }
                }
            }
            let description_x = row.x + 18;
            if description_x < row.right() {
                buffer.set_string(
                    description_x,
                    y,
                    suggestion.description,
                    if selected {
                        row_style
                    } else {
                        Style::default().fg(theme.gray).bg(theme.bg_base)
                    },
                );
            }
        }
    }
}

fn set(buffer: &mut Buffer, x: u16, y: u16, character: char, style: Style) {
    if let Some(cell) = buffer.cell_mut((x, y)) {
        cell.set_char(character);
        cell.set_style(style);
    }
}
