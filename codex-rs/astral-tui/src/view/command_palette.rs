use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;

use crate::command_palette::CommandPaletteEntry;
use crate::command_palette::CommandPaletteState;
use crate::modal::ModalRowHit;

use super::AstralTheme;
use super::modal::ModalSizing;
use super::modal::modal_choice_style;
use super::modal::render_modal_close_button;
use super::modal::render_modal_frame_with_sizing;

pub(crate) struct CommandPalette<'a> {
    pub(crate) state: &'a mut CommandPaletteState,
}

impl CommandPalette<'_> {
    pub(crate) fn render(self, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
        let Some(frame) = render_modal_frame_with_sizing(
            area,
            buffer,
            theme,
            "Commands",
            "type to search · ↑/↓ navigate · Enter run · Esc close",
            ModalSizing::shortcuts(),
        ) else {
            return;
        };
        render_modal_close_button(
            buffer,
            frame.close_button,
            theme,
            self.state.pointer.close_hovered(),
        );
        let content = frame.content;
        if content.is_empty() {
            self.state
                .pointer
                .observe_frame(frame.popup, frame.close_button, Vec::new());
            return;
        }
        let query = if self.state.query().is_empty() {
            "Search commands"
        } else {
            self.state.query()
        };
        buffer.set_stringn(
            content.x,
            content.y,
            format!("› {query}"),
            usize::from(content.width),
            Style::default()
                .fg(if self.state.query().is_empty() {
                    theme.gray
                } else {
                    theme.text_primary
                })
                .bg(theme.bg_base),
        );
        if content.height > 1 {
            buffer.set_stringn(
                content.x,
                content.y.saturating_add(1),
                "─".repeat(usize::from(content.width)),
                usize::from(content.width),
                Style::default().fg(theme.gray_dim).bg(theme.bg_base),
            );
        }
        let list = Rect::new(
            content.x,
            content.y.saturating_add(2),
            content.width,
            content.height.saturating_sub(2),
        );
        let visible = self.state.visible_indices();
        self.state
            .ensure_selection_visible(usize::from(list.height));
        if visible.is_empty() {
            buffer.set_stringn(
                list.x,
                list.y,
                "No matching commands",
                usize::from(list.width),
                Style::default().fg(theme.gray).bg(theme.bg_base),
            );
            self.state
                .pointer
                .observe_frame(frame.popup, frame.close_button, Vec::new());
            return;
        }
        let mut rows = Vec::new();
        for (row, entry_index) in visible
            .iter()
            .copied()
            .enumerate()
            .skip(self.state.scroll_offset)
            .take(usize::from(list.height))
        {
            let y = list.y
                + u16::try_from(row.saturating_sub(self.state.scroll_offset)).unwrap_or(u16::MAX);
            let row_area = Rect::new(list.x, y, list.width, 1);
            let Some(entry) = self.state.entry(entry_index) else {
                continue;
            };
            match entry {
                CommandPaletteEntry::Section(label) => {
                    buffer.set_stringn(
                        row_area.x,
                        row_area.y,
                        format!("─ {label}"),
                        usize::from(row_area.width),
                        Style::default()
                            .fg(theme.gray)
                            .bg(theme.bg_base)
                            .add_modifier(Modifier::BOLD),
                    );
                }
                CommandPaletteEntry::Command {
                    label, shortcut, ..
                } => {
                    let selected = self.state.selected() == row
                        || self.state.pointer.hovered_row() == Some(row);
                    let style = modal_choice_style(theme, selected);
                    buffer.set_style(row_area, style);
                    let shortcut_width =
                        u16::try_from(Line::from(shortcut.as_str()).width()).unwrap_or(u16::MAX);
                    let shortcut_x = row_area
                        .right()
                        .saturating_sub(shortcut_width)
                        .saturating_sub(1);
                    let label_x = row_area.x.saturating_add(1);
                    buffer.set_stringn(
                        label_x,
                        row_area.y,
                        label,
                        usize::from(shortcut_x.saturating_sub(label_x).saturating_sub(2)),
                        style,
                    );
                    if shortcut_width.saturating_add(2) < row_area.width {
                        buffer.set_stringn(
                            shortcut_x,
                            row_area.y,
                            shortcut,
                            usize::from(shortcut_width),
                            Style::default()
                                .fg(if selected {
                                    theme.text_primary
                                } else {
                                    theme.gray
                                })
                                .bg(if selected {
                                    theme.panel_selected
                                } else {
                                    theme.bg_base
                                }),
                        );
                    }
                    rows.push(ModalRowHit {
                        id: row,
                        area: row_area,
                    });
                }
            }
        }
        self.state
            .pointer
            .observe_frame(frame.popup, frame.close_button, rows);
    }
}
