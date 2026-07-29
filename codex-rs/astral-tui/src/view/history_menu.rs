use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;

use crate::history::HistorySnapshot;

use super::AstralTheme;
use super::CompletionMenuFrame;

pub(crate) struct HistoryMenu<'a> {
    pub(crate) snapshot: &'a HistorySnapshot,
    pub(crate) hovered: Option<usize>,
}

impl HistoryMenu<'_> {
    pub(crate) fn render(
        self,
        area: Rect,
        buffer: &mut Buffer,
        theme: AstralTheme,
    ) -> CompletionMenuFrame {
        let mut frame =
            CompletionMenuFrame::new(area, self.snapshot.matches.len(), self.snapshot.selected);
        if area.width < 12 || area.height < 3 {
            return frame;
        }
        let border = theme.prompt_border_active;
        let background = theme.bg_base;
        buffer.set_style(area, Style::default().fg(theme.text_primary).bg(background));
        render_border(area, buffer, border, background);
        buffer.set_string(
            area.x + 2,
            area.y,
            " history ",
            Style::default().fg(theme.gray).bg(background),
        );
        let count = self.snapshot.matches.len().to_string();
        let count_width = u16::try_from(Line::from(count.as_str()).width()).unwrap_or(u16::MAX);
        if count_width + 3 < area.width {
            buffer.set_string(
                area.right().saturating_sub(count_width + 2),
                area.y,
                count,
                Style::default().fg(theme.gray).bg(background),
            );
        }

        if self.snapshot.matches.is_empty() {
            let row = frame.row_rect(0);
            buffer.set_stringn(
                row.x,
                row.y,
                "  no matching history",
                usize::from(row.width),
                Style::default().fg(theme.gray).bg(background),
            );
            return frame;
        }

        let rows = frame.visible_rows();
        let start = frame.window_start();
        for (visible, (index, entry)) in self
            .snapshot
            .matches
            .iter()
            .enumerate()
            .skip(start)
            .take(rows)
            .enumerate()
        {
            let row = frame.row_rect(visible);
            frame.observe_row(index, row);
            let selected = index == self.snapshot.selected;
            let hovered = self.hovered == Some(index);
            let row_background = if selected {
                theme.text_secondary
            } else if hovered {
                theme.panel_selected
            } else {
                background
            };
            let row_style = if selected {
                Style::default().fg(background).bg(row_background)
            } else {
                Style::default().fg(theme.text_primary).bg(row_background)
            };
            buffer.set_style(row, row_style);
            buffer.set_string(row.x, row.y, if selected { "› " } else { "  " }, row_style);
            let text_x = row.x.saturating_add(2);
            let text_width = usize::from(row.right().saturating_sub(text_x));
            buffer.set_stringn(text_x, row.y, &entry.display, text_width, row_style);
            if !selected {
                for index in &entry.indices {
                    let prefix = entry.display.chars().take(*index).collect::<String>();
                    let x = text_x.saturating_add(
                        u16::try_from(Line::from(prefix).width()).unwrap_or(u16::MAX),
                    );
                    if x < row.right()
                        && let Some(cell) = buffer.cell_mut((x, row.y))
                    {
                        cell.set_style(
                            Style::default()
                                .fg(theme.accent_running)
                                .bg(row_background)
                                .add_modifier(Modifier::BOLD),
                        );
                    }
                }
            }
        }
        frame.render_scrollbar(
            buffer,
            theme,
            self.snapshot.matches.len(),
            self.snapshot.selected,
        );
        frame
    }
}

fn render_border(
    area: Rect,
    buffer: &mut Buffer,
    foreground: ratatui::style::Color,
    background: ratatui::style::Color,
) {
    let style = Style::default().fg(foreground).bg(background);
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
            style,
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
            style,
        );
    }
    for y in area.y + 1..area.bottom() - 1 {
        set(buffer, area.x, y, '│', style);
        set(buffer, area.right() - 1, y, '│', style);
    }
}

fn set(buffer: &mut Buffer, x: u16, y: u16, character: char, style: Style) {
    if let Some(cell) = buffer.cell_mut((x, y)) {
        cell.set_char(character).set_style(style);
    }
}
