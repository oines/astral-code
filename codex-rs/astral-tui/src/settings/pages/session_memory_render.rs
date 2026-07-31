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
use crate::view::ModalSizing;
use crate::view::modal_choice_style;
use crate::view::render_modal_close_button;
use crate::view::render_modal_frame_with_geometry;
use crate::view::render_modal_frame_with_sizing;

use super::super::BACK_ROW_ID;
use super::super::SettingsState;
use super::super::render_row::truncate_to_width;
use super::session_memory::MemoryEditor;
use super::session_memory::MemoryField;
use super::session_memory::TemplateSource;

pub(super) fn render(
    state: &mut SettingsState,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    if state.session_memory.editor.is_some() {
        render_editor(state, area, buffer, theme);
    } else {
        render_form(state, area, buffer, theme);
    }
}

fn render_form(state: &mut SettingsState, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
    let Some(frame) = render_modal_frame_with_sizing(
        area,
        buffer,
        theme,
        "Session Memory Templates",
        "j/k · Enter edit · Ctrl+S save · Esc back",
        ModalSizing::settings(),
    ) else {
        return;
    };
    render_modal_close_button(
        buffer,
        frame.close_button,
        theme,
        state.pointer.close_hovered(),
    );
    let content = frame.content;
    let breadcrumb = Rect::new(content.x, content.y, content.width, 1);
    let breadcrumb_style = if state.pointer.hovered_row() == Some(BACK_ROW_ID) {
        Style::default()
            .fg(theme.accent_running)
            .bg(theme.bg_base)
            .underlined()
    } else {
        Style::default().fg(theme.gray).bg(theme.bg_base)
    };
    buffer.set_stringn(
        breadcrumb.x,
        breadcrumb.y,
        "‹ Settings  /  Advanced  /  Session Memory Templates",
        usize::from(breadcrumb.width),
        breadcrumb_style,
    );
    let details = Rect::new(
        content.x,
        content.y.saturating_add(2),
        content.width,
        3.min(content.height.saturating_sub(4)),
    );
    let selected_field = state.session_memory.field();
    let metadata = memory_metadata(state, selected_field);
    for (index, (text, style)) in [
        (
            state.session_memory.description(selected_field),
            Style::default().fg(theme.text_primary).bg(theme.bg_base),
        ),
        (
            metadata.as_str(),
            Style::default().fg(theme.gray).bg(theme.bg_base),
        ),
        (
            "Inline clears File, and File clears Inline, on Save.",
            Style::default().fg(theme.gray).bg(theme.bg_base),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let y = details
            .y
            .saturating_add(u16::try_from(index).unwrap_or(u16::MAX));
        if y >= details.bottom() {
            break;
        }
        buffer.set_stringn(
            details.x,
            y,
            truncate_to_width(text, usize::from(details.width)),
            usize::from(details.width),
            style,
        );
    }
    let list_y = details.bottom().saturating_add(1);
    let message = state
        .session_memory
        .error
        .clone()
        .map(|message| (message, true))
        .or_else(|| {
            state
                .notice()
                .map(|message| (message.to_string(), state.notice_is_error()))
        });
    let error_height = u16::from(message.is_some());
    let save_index = MemoryField::ALL.len() - 1;
    let save_y = content
        .bottom()
        .saturating_sub(error_height)
        .saturating_sub(1);
    let list = Rect::new(
        content.x,
        list_y,
        content.width,
        save_y.saturating_sub(list_y),
    );
    let visible_rows = usize::from(list.height);
    if state.session_memory.selected != save_index && visible_rows > 0 {
        if state.session_memory.selected < state.session_memory.scroll_offset {
            state.session_memory.scroll_offset = state.session_memory.selected;
        }
        if state.session_memory.selected
            >= state
                .session_memory
                .scroll_offset
                .saturating_add(visible_rows)
        {
            state.session_memory.scroll_offset = state
                .session_memory
                .selected
                .saturating_add(1)
                .saturating_sub(visible_rows);
        }
    }
    let mut hits = vec![ModalRowHit {
        id: BACK_ROW_ID,
        area: breadcrumb,
    }];
    for (index, field) in MemoryField::ALL[..save_index]
        .iter()
        .copied()
        .enumerate()
        .skip(state.session_memory.scroll_offset)
        .take(visible_rows)
    {
        let y = list.y
            + u16::try_from(index.saturating_sub(state.session_memory.scroll_offset))
                .unwrap_or(u16::MAX);
        if y >= list.bottom() {
            break;
        }
        let row = Rect::new(list.x, y, list.width, 1);
        let selected =
            index == state.session_memory.selected || state.pointer.hovered_row() == Some(index);
        render_field(state, field, row, buffer, theme, selected);
        hits.push(ModalRowHit {
            id: index,
            area: row,
        });
    }
    if save_y >= list_y && save_y < content.bottom() {
        let row = Rect::new(content.x, save_y, content.width, 1);
        let selected = state.session_memory.selected == save_index
            || state.pointer.hovered_row() == Some(save_index);
        render_field(state, MemoryField::Save, row, buffer, theme, selected);
        hits.push(ModalRowHit {
            id: save_index,
            area: row,
        });
    }
    if let Some((message, is_error)) = message {
        buffer.set_stringn(
            content.x,
            content.bottom().saturating_sub(1),
            &message,
            usize::from(content.width),
            Style::default()
                .fg(if is_error {
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

fn render_field(
    state: &SettingsState,
    field: MemoryField,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
    selected: bool,
) {
    let style = modal_choice_style(theme, selected);
    buffer.set_style(area, style);
    let value = state.session_memory.value(field);
    let value = truncate_to_width(&value, usize::from(area.width / 2));
    let value_width = Line::from(value.as_str()).width();
    let x = area
        .right()
        .saturating_sub(u16::try_from(value_width).unwrap_or(area.width));
    let label_x = area.x.saturating_add(4);
    let label_width = usize::from(x.saturating_sub(label_x).saturating_sub(1));
    let label = truncate_to_width(field.label(), label_width);
    let marker = if field == MemoryField::Save {
        "◆"
    } else {
        "›"
    };
    buffer.set_stringn(
        area.x,
        area.y,
        format!("{} {marker} {label}", if selected { "❯" } else { " " }),
        usize::from(area.width),
        style,
    );
    buffer.set_stringn(
        x,
        area.y,
        value,
        value_width,
        Style::default()
            .fg(if selected {
                theme.accent_running
            } else {
                theme.gray
            })
            .bg(style.bg.unwrap_or(theme.bg_base)),
    );
}

fn memory_metadata(state: &SettingsState, field: MemoryField) -> String {
    let key = match field {
        MemoryField::SummarySource | MemoryField::SummaryValue => {
            match state.session_memory.summary_source {
                TemplateSource::BuiltIn => None,
                TemplateSource::Inline => Some("session_memory_template"),
                TemplateSource::File => Some("experimental_session_memory_template_file"),
            }
        }
        MemoryField::UpdateSource | MemoryField::UpdateValue => {
            match state.session_memory.update_source {
                TemplateSource::BuiltIn => None,
                TemplateSource::Inline => Some("session_memory_update_prompt"),
                TemplateSource::File => Some("experimental_session_memory_update_prompt_file"),
            }
        }
        MemoryField::Save => return "User config · next extraction".to_string(),
    };
    key.map_or_else(
        || "Built in · next extraction".to_string(),
        |key| {
            let overridden = if state.store.is_overridden_above_user(key) {
                " · user value overridden"
            } else {
                Default::default()
            };
            format!(
                "{key} · {} · next extraction{overridden}",
                state.store.source_label(key)
            )
        },
    )
}

fn render_editor(state: &mut SettingsState, area: Rect, buffer: &mut Buffer, theme: AstralTheme) {
    let editor = state.session_memory.editor.clone();
    let Some(editor) = editor else {
        return;
    };
    let (title, footer, height) = match &editor {
        MemoryEditor::Text { field, .. } => (field.label(), "Enter apply · Esc cancel · paste", 12),
        MemoryEditor::Picker { field, .. } => {
            (field.label(), "j/k choose · Enter apply · Esc cancel", 8)
        }
    };
    let Some(frame) = render_modal_frame_with_geometry(
        area,
        buffer,
        theme,
        title,
        footer,
        ModalHeight::MinimumContent(height),
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
    match editor {
        MemoryEditor::Text { field, input } => {
            Paragraph::new(state.session_memory.description(field))
                .wrap(ratatui::widgets::Wrap { trim: false })
                .render(
                    Rect::new(frame.content.x, frame.content.y, frame.content.width, 2),
                    buffer,
                );
            let field_area = Rect::new(
                frame.content.x,
                frame.content.y.saturating_add(3),
                frame.content.width,
                frame
                    .content
                    .height
                    .saturating_sub(
                        6 + u16::from(
                            state.session_memory.error.is_some() || state.notice().is_some(),
                        ),
                    )
                    .max(1),
            );
            buffer.set_style(field_area, Style::default().bg(theme.panel_background));
            let text = input.text();
            let cursor = input.cursor().min(text.len());
            Paragraph::new(Line::from(vec![
                "  ".into(),
                text[..cursor].into(),
                "▏".fg(theme.accent_running),
                text[cursor..].into(),
            ]))
            .wrap(ratatui::widgets::Wrap { trim: false })
            .render(field_area, buffer);
            let actions_y = field_area.bottom().saturating_add(1);
            for (index, label) in ["Apply to draft", "Cancel"].iter().enumerate() {
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
            let message = state
                .session_memory
                .error
                .as_deref()
                .map(|message| (message, true))
                .or_else(|| {
                    state
                        .notice()
                        .map(|message| (message, state.notice_is_error()))
                });
            if let Some((message, is_error)) = message {
                buffer.set_stringn(
                    frame.content.x,
                    frame.content.bottom().saturating_sub(1),
                    message,
                    usize::from(frame.content.width),
                    Style::default()
                        .fg(if is_error {
                            theme.accent_error
                        } else {
                            theme.accent_running
                        })
                        .bg(theme.bg_base),
                );
            }
        }
        MemoryEditor::Picker { selected, .. } => {
            let visible_rows = usize::from(frame.content.height).max(1);
            let start = selected.saturating_add(1).saturating_sub(visible_rows);
            for (index, source) in TemplateSource::ALL
                .iter()
                .copied()
                .enumerate()
                .skip(start)
                .take(visible_rows)
            {
                let y = frame.content.y
                    + u16::try_from(index.saturating_sub(start)).unwrap_or(u16::MAX);
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
                        source.label()
                    ),
                    usize::from(row.width),
                    style,
                );
                hits.push(ModalRowHit {
                    id: index,
                    area: row,
                });
            }
        }
    }
    state
        .pointer
        .observe_frame(frame.popup, frame.close_button, hits);
}
