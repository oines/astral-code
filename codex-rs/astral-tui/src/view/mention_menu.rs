use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;

use crate::mention::MentionKind;
use crate::mention::MentionSnapshot;

use super::AstralTheme;
use super::CompletionMenuFrame;

pub(crate) struct MentionMenu<'a> {
    pub(crate) snapshot: &'a MentionSnapshot,
    pub(crate) hovered: Option<usize>,
}

impl MentionMenu<'_> {
    pub(crate) fn render(
        self,
        area: Rect,
        buffer: &mut Buffer,
        theme: AstralTheme,
    ) -> CompletionMenuFrame {
        let mut frame =
            CompletionMenuFrame::new(area, self.snapshot.matches.len(), self.snapshot.selected);
        if area.width < 20 || area.height < 3 {
            return frame;
        }
        let border = theme.prompt_border_active;
        buffer.set_style(
            area,
            Style::default().fg(theme.text_primary).bg(theme.bg_base),
        );
        render_border(area, buffer, border, theme.bg_base);
        buffer.set_string(
            area.x + 2,
            area.y,
            " skills & plugins ",
            Style::default().fg(theme.gray).bg(theme.bg_base),
        );

        let rows = frame.visible_rows();
        let start = frame.window_start();
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
            let hovered = self.hovered == Some(index);
            let row_background = if selected {
                theme.text_secondary
            } else if hovered {
                theme.panel_selected
            } else {
                theme.bg_base
            };
            let row_style = if selected {
                Style::default().fg(theme.bg_base).bg(row_background)
            } else {
                Style::default().fg(theme.text_primary).bg(row_background)
            };
            let row = frame.row_rect(visible);
            frame.observe_row(index, row);
            buffer.set_style(row, row_style);
            buffer.set_string(row.x, y, if selected { "› " } else { "  " }, row_style);

            let tag_style = if selected {
                row_style
            } else {
                match suggestion.kind {
                    MentionKind::Plugin => {
                        Style::default().fg(theme.accent_running).bg(row_background)
                    }
                    MentionKind::Skill => Style::default().fg(theme.gray).bg(row_background),
                }
            };
            let tag = format!("{:<7}", suggestion.kind.label());
            buffer.set_string(row.x + 2, y, tag, tag_style);
            let display_x = row.x + 10;
            buffer.set_string(display_x, y, &suggestion.display, row_style);
            if !selected {
                for index in &suggestion.indices {
                    let prefix = suggestion.display.chars().take(*index).collect::<String>();
                    let x =
                        display_x + u16::try_from(Line::from(prefix).width()).unwrap_or(u16::MAX);
                    if let Some(cell) = buffer.cell_mut((x, y)) {
                        cell.set_style(
                            Style::default()
                                .fg(theme.accent_running)
                                .bg(row_background)
                                .add_modifier(Modifier::BOLD),
                        );
                    }
                }
            }

            let display_width =
                u16::try_from(Line::from(suggestion.display.as_str()).width()).unwrap_or(u16::MAX);
            let description_x = (display_x + display_width + 2).max(row.x + 30);
            if description_x < row.right() {
                buffer.set_string(
                    description_x,
                    y,
                    &suggestion.description,
                    if selected {
                        row_style
                    } else {
                        Style::default().fg(theme.gray).bg(row_background)
                    },
                );
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
}

fn set(buffer: &mut Buffer, x: u16, y: u16, character: char, style: Style) {
    if let Some(cell) = buffer.cell_mut((x, y)) {
        cell.set_char(character);
        cell.set_style(style);
    }
}
