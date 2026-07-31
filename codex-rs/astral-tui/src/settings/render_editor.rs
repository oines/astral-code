use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::modal::ModalRowHit;
use crate::view::AstralTheme;
use crate::view::ModalHeight;
use crate::view::modal_choice_style;
use crate::view::render_modal_close_button;
use crate::view::render_modal_frame_with_geometry;

use super::SettingsEditor;
use super::SettingsState;
use super::render_row::render_wrapped_line;

pub(super) fn render(
    state: &mut SettingsState,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    let editor = state.editor.clone();
    let Some(editor) = editor else {
        return;
    };
    match editor {
        SettingsEditor::Text { definition, input } => {
            let Some(frame) = render_modal_frame_with_geometry(
                area,
                buffer,
                theme,
                definition.label,
                "Enter save · Esc cancel · paste",
                ModalHeight::MinimumContent(10),
            ) else {
                return;
            };
            render_modal_close_button(
                buffer,
                frame.close_button,
                theme,
                state.pointer.close_hovered(),
            );
            render_wrapped_line(
                Rect::new(frame.content.x, frame.content.y, frame.content.width, 2),
                buffer,
                definition.description,
                Style::default().fg(theme.gray).bg(theme.bg_base),
            );
            let field = Rect::new(
                frame.content.x,
                frame.content.y.saturating_add(3),
                frame.content.width,
                frame
                    .content
                    .height
                    .saturating_sub(6 + u16::from(state.notice().is_some()))
                    .max(1),
            );
            buffer.set_style(field, Style::default().bg(theme.panel_background));
            let text = input.text();
            let cursor = input.cursor().min(text.len());
            Paragraph::new(Line::from(vec![
                "  ".into(),
                text[..cursor].into(),
                "▏".fg(theme.accent_running),
                text[cursor..].into(),
            ]))
            .wrap(ratatui::widgets::Wrap { trim: false })
            .render(field, buffer);
            let mut hits = Vec::new();
            let actions_y = field.bottom().saturating_add(1);
            for (index, label) in ["Save", "Cancel"].iter().enumerate() {
                let y = actions_y.saturating_add(u16::try_from(index).unwrap_or_default());
                if y >= frame.content.bottom() {
                    break;
                }
                let row = Rect::new(frame.content.x, y, frame.content.width, 1);
                let hovered = state.pointer.hovered_row() == Some(index);
                let style = modal_choice_style(theme, hovered);
                buffer.set_style(row, style);
                buffer.set_stringn(
                    row.x,
                    row.y,
                    format!("{} {label}", if hovered { "❯" } else { " " }),
                    usize::from(row.width),
                    style,
                );
                hits.push(ModalRowHit {
                    id: index,
                    area: row,
                });
            }
            if let Some(notice) = state.notice() {
                buffer.set_stringn(
                    frame.content.x,
                    frame.content.bottom().saturating_sub(1),
                    notice,
                    usize::from(frame.content.width),
                    Style::default()
                        .fg(if state.notice_is_error() {
                            theme.accent_error
                        } else {
                            theme.accent_running
                        })
                        .bg(theme.bg_base),
                );
            }
            state
                .pointer
                .observe_frame(frame.popup, frame.close_button, hits);
        }
        SettingsEditor::Picker {
            definition,
            options,
            selected,
            ..
        } => {
            let title = definition.map_or("Choose value", |definition| definition.label);
            let Some(frame) = render_modal_frame_with_geometry(
                area,
                buffer,
                theme,
                title,
                "j/k preview · Enter save · Esc cancel",
                ModalHeight::MinimumContent(u16::try_from(options.len()).unwrap_or(12)),
            ) else {
                return;
            };
            render_modal_close_button(
                buffer,
                frame.close_button,
                theme,
                state.pointer.close_hovered(),
            );
            let mut hits = Vec::new();
            let visible_rows = usize::from(frame.content.height).max(1);
            let start = selected.saturating_add(1).saturating_sub(visible_rows);
            for (index, option) in options.iter().enumerate().skip(start).take(visible_rows) {
                let y = frame.content.y
                    + u16::try_from(index.saturating_sub(start)).unwrap_or(u16::MAX);
                if y >= frame.content.bottom() {
                    break;
                }
                let row = Rect::new(frame.content.x, y, frame.content.width, 1);
                let active = index == selected || state.pointer.hovered_row() == Some(index);
                let style = modal_choice_style(theme, active);
                buffer.set_style(row, style);
                buffer.set_stringn(
                    row.x,
                    row.y,
                    format!(
                        "{} {}",
                        if index == selected { "❯" } else { " " },
                        option.label
                    ),
                    usize::from(row.width),
                    style,
                );
                hits.push(ModalRowHit {
                    id: index,
                    area: row,
                });
            }
            state
                .pointer
                .observe_frame(frame.popup, frame.close_button, hits);
        }
        SettingsEditor::Confirm {
            title,
            message,
            confirm_label,
            ..
        } => {
            let Some(frame) = render_modal_frame_with_geometry(
                area,
                buffer,
                theme,
                &title,
                "Enter/y confirm · Esc/n cancel",
                ModalHeight::MinimumContent(8),
            ) else {
                return;
            };
            render_modal_close_button(
                buffer,
                frame.close_button,
                theme,
                state.pointer.close_hovered(),
            );
            let message_area = Rect::new(
                frame.content.x,
                frame.content.y,
                frame.content.width,
                3.min(frame.content.height),
            );
            Paragraph::new(message)
                .wrap(ratatui::widgets::Wrap { trim: false })
                .render(message_area, buffer);
            let choices_y = message_area.bottom().saturating_add(1);
            let mut hits = Vec::new();
            for (index, label) in [confirm_label.as_str(), "Cancel"].iter().enumerate() {
                let y = choices_y + u16::try_from(index).unwrap_or_default();
                if y >= frame.content.bottom() {
                    break;
                }
                let row = Rect::new(frame.content.x, y, frame.content.width, 1);
                let active = state.pointer.hovered_row().unwrap_or_default() == index;
                let style = modal_choice_style(theme, active);
                buffer.set_style(row, style);
                buffer.set_stringn(
                    row.x,
                    row.y,
                    format!("{} {label}", if active { "❯" } else { " " }),
                    usize::from(row.width),
                    style,
                );
                hits.push(ModalRowHit {
                    id: index,
                    area: row,
                });
            }
            state
                .pointer
                .observe_frame(frame.popup, frame.close_button, hits);
        }
    }
}
