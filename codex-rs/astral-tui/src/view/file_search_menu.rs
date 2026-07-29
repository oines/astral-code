use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::BorderType;
use ratatui::widgets::Borders;
use ratatui::widgets::Widget;

use crate::file_search::FileSearchSnapshot;

use super::AstralTheme;
use super::CompletionMenuFrame;

pub(crate) struct FileSearchMenu<'a> {
    pub(crate) snapshot: &'a FileSearchSnapshot,
    pub(crate) hovered: Option<usize>,
}

impl FileSearchMenu<'_> {
    pub(crate) fn render(
        self,
        area: Rect,
        buffer: &mut Buffer,
        theme: AstralTheme,
    ) -> CompletionMenuFrame {
        let item_count = self.snapshot.matches.len();
        let mut frame = CompletionMenuFrame::new(area, item_count.max(1), self.snapshot.selected);
        if area.width < 12 || area.height < 3 {
            return frame;
        }
        let border = theme.prompt_border_active;
        let background = theme.bg_base;
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border).bg(background))
            .style(Style::default().fg(theme.text_primary).bg(background))
            .title_style(Style::default().fg(theme.gray).bg(background))
            .title(" files ")
            .render(area, buffer);
        let count = item_count.to_string();
        let count_width = u16::try_from(count.len()).unwrap_or(u16::MAX);
        if count_width + 3 < area.width {
            buffer.set_string(
                area.right().saturating_sub(count_width + 2),
                area.y,
                count,
                Style::default().fg(theme.gray).bg(background),
            );
        }

        if self.snapshot.matches.is_empty() {
            let message = if self.snapshot.query.is_empty() {
                "  type after @ to search files"
            } else if self.snapshot.waiting {
                "  searching…"
            } else if self.snapshot.error.is_some() {
                "  file search unavailable"
            } else {
                "  no matching files"
            };
            let row = frame.row_rect(0);
            buffer.set_stringn(
                row.x,
                row.y,
                message,
                usize::from(row.width),
                Style::default().fg(theme.gray).bg(background),
            );
            return frame;
        }

        let rows = frame.visible_rows();
        let start = frame.window_start();
        for (visible, (index, result)) in self
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

            let path_x = row.x + 2;
            let path = result.path.strip_prefix("./").unwrap_or(&result.path);
            let path_width = usize::from(row.right().saturating_sub(path_x));
            buffer.set_stringn(path_x, row.y, path, path_width, row_style);
            if !selected {
                for index in result.indices.as_deref().unwrap_or_default() {
                    let prefix = path.chars().take(*index as usize).collect::<String>();
                    let x = path_x.saturating_add(
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
