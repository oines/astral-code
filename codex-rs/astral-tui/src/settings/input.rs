use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;

use crate::modal::ModalPointerAction;

use super::BACK_ROW_ID;
use super::SEARCH_ROW_ID;
use super::SettingKind;
use super::SettingsConfirmAction;
use super::SettingsEditor;
use super::SettingsFocus;
use super::SettingsInput;
use super::SettingsPage;
use super::SettingsRow;
use super::SettingsState;
use super::pages::models::ModelsManagerInput;

pub(crate) fn handle_key(state: &mut SettingsState, key: KeyEvent) -> SettingsInput {
    if key.kind == KeyEventKind::Release {
        return SettingsInput::None;
    }
    if key.kind == KeyEventKind::Repeat && matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')) {
        return SettingsInput::None;
    }
    state.pointer.clear_hover();
    state.clear_notice();
    if state.editor.is_some() {
        return super::input_editor::handle_key(state, key);
    }
    if state.page == SettingsPage::Models {
        let input = super::pages::models::handle_key(&mut state.models, key);
        return apply_models_input(state, input);
    }
    if state.page == SettingsPage::Search {
        return super::pages::handle_key(state, key);
    }
    if state.page == SettingsPage::SessionMemoryTemplates {
        return super::pages::handle_key(state, key);
    }
    if state.search_focused {
        return match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                state.clear_query();
                state.focus_list();
                SettingsInput::Redraw
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                state.focus_list();
                SettingsInput::Redraw
            }
            (KeyCode::Up, _) => {
                state.move_selection(-1);
                SettingsInput::Redraw
            }
            (KeyCode::Down, _) => {
                state.move_selection(1);
                SettingsInput::Redraw
            }
            (KeyCode::PageUp, _) => {
                state.move_selection(-10);
                SettingsInput::Redraw
            }
            (KeyCode::PageDown, _) => {
                state.move_selection(10);
                SettingsInput::Redraw
            }
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                state.clear_query();
                SettingsInput::Redraw
            }
            _ if state.edit_query(key) => {
                state.selected = 0;
                state.scroll_offset = 0;
                SettingsInput::Redraw
            }
            _ => SettingsInput::None,
        };
    }
    match (key.code, key.modifiers) {
        (KeyCode::Esc, _) => {
            if state.clear_query() {
                SettingsInput::Redraw
            } else {
                state.request_back()
            }
        }
        (KeyCode::Up | KeyCode::Char('k'), _) => {
            state.move_selection(-1);
            SettingsInput::Redraw
        }
        (KeyCode::Down | KeyCode::Char('j'), _) => {
            state.move_selection(1);
            SettingsInput::Redraw
        }
        (KeyCode::PageUp, _) => {
            state.move_selection(-10);
            SettingsInput::Redraw
        }
        (KeyCode::PageDown, _) => {
            state.move_selection(10);
            SettingsInput::Redraw
        }
        (KeyCode::Home | KeyCode::Char('g'), _) => {
            state.set_selected(0);
            SettingsInput::Redraw
        }
        (KeyCode::End | KeyCode::Char('G'), _) => {
            state.set_selected(usize::MAX);
            SettingsInput::Redraw
        }
        (KeyCode::Enter, KeyModifiers::NONE) => state.activate_selected(),
        (KeyCode::Char(' '), KeyModifiers::NONE) => match state.selected_row() {
            Some(SettingsRow::Definition(definition))
                if matches!(definition.kind, SettingKind::Bool) =>
            {
                state.activate_selected()
            }
            Some(SettingsRow::Feature(_)) => state.activate_selected(),
            Some(SettingsRow::Category(_) | SettingsRow::Definition(_)) | None => {
                SettingsInput::None
            }
        },
        (KeyCode::Right | KeyCode::Char('l'), _) => {
            let Some(row) = state.selected_row() else {
                return SettingsInput::None;
            };
            let enters_page = matches!(row, SettingsRow::Category(_))
                || matches!(
                    row,
                    SettingsRow::Definition(definition)
                        if matches!(definition.kind, SettingKind::Subpage(_))
                );
            if enters_page {
                state.activate(row)
            } else {
                state.toggle_expanded(row);
                SettingsInput::Redraw
            }
        }
        (KeyCode::Left | KeyCode::Char('h'), _) => {
            if let Some(row) = state.selected_row()
                && state.row_expanded(row)
            {
                state.toggle_expanded(row);
                return SettingsInput::Redraw;
            }
            state.request_back()
        }
        (KeyCode::Char('/') | KeyCode::Char('i'), KeyModifiers::NONE) => {
            state.focus_search();
            SettingsInput::Redraw
        }
        (KeyCode::Backspace, _) if !state.query.text().is_empty() => {
            state.focus_search();
            state.edit_query(key);
            SettingsInput::Redraw
        }
        (KeyCode::Char('d'), KeyModifiers::NONE) => state.reset_selected(),
        _ => SettingsInput::None,
    }
}

pub(crate) fn handle_paste(state: &mut SettingsState, text: &str) -> SettingsInput {
    state.clear_notice();
    if let Some(SettingsEditor::Text { input, .. }) = state.editor.as_mut() {
        input.insert_text(text);
        return SettingsInput::Redraw;
    }
    if state.editor.is_some() {
        return SettingsInput::None;
    }
    if state.page == SettingsPage::Models {
        let input = super::pages::models::handle_paste(&mut state.models, text);
        return apply_models_input(state, input);
    }
    if state.page == SettingsPage::Search {
        return super::pages::handle_paste(state, text);
    }
    if state.page == SettingsPage::SessionMemoryTemplates {
        return super::pages::handle_paste(state, text);
    }
    state.focus_search();
    state.paste_query(text);
    SettingsInput::Redraw
}

pub(crate) fn handle_mouse(state: &mut SettingsState, mouse: MouseEvent) -> SettingsInput {
    if state.editor.is_some() {
        return super::input_editor::handle_mouse(state, mouse);
    }
    if state.page == SettingsPage::Models {
        let input = super::pages::models::handle_mouse(&mut state.models, mouse);
        return apply_models_input(state, input);
    }
    if matches!(
        state.page,
        SettingsPage::Search | SettingsPage::SessionMemoryTemplates
    ) {
        return super::pages::handle_mouse(state, mouse);
    }
    let position = (mouse.column, mouse.row);
    match state.pointer.handle_mouse(mouse) {
        ModalPointerAction::Ignored => SettingsInput::None,
        ModalPointerAction::Redraw | ModalPointerAction::Hover(None) => SettingsInput::Redraw,
        ModalPointerAction::Close => {
            state.clear_notice();
            state.request_close()
        }
        ModalPointerAction::Hover(Some(_)) => SettingsInput::Redraw,
        ModalPointerAction::Activate(index) => {
            state.clear_notice();
            if index == SEARCH_ROW_ID {
                state.focus_search();
                return SettingsInput::Redraw;
            }
            if index == BACK_ROW_ID {
                return state.request_back();
            }
            if state
                .row_expand_hits
                .get(index)
                .and_then(Option::as_ref)
                .is_some_and(|area| contains(*area, position))
            {
                state.set_selected(index);
                if let Some(row) = state.selected_row() {
                    state.toggle_expanded(row);
                }
                return SettingsInput::Redraw;
            }
            if state
                .row_value_hits
                .get(index)
                .and_then(Option::as_ref)
                .is_some_and(|area| contains(*area, position))
            {
                state.set_selected(index);
                return state.activate_selected();
            }
            if state.selected != index {
                state.set_selected(index);
                SettingsInput::Redraw
            } else {
                state.activate_selected()
            }
        }
        ModalPointerAction::Scroll(delta) => {
            state.clear_notice();
            state.move_selection(delta);
            SettingsInput::Redraw
        }
    }
}

fn contains(area: ratatui::layout::Rect, position: (u16, u16)) -> bool {
    position.0 >= area.x
        && position.0 < area.right()
        && position.1 >= area.y
        && position.1 < area.bottom()
}

fn apply_models_input(state: &mut SettingsState, input: ModelsManagerInput) -> SettingsInput {
    match input {
        ModelsManagerInput::None => SettingsInput::None,
        ModelsManagerInput::Redraw => SettingsInput::Redraw,
        ModelsManagerInput::Cancel => {
            state.go_back();
            SettingsInput::Redraw
        }
        ModelsManagerInput::Close => state.request_close(),
        ModelsManagerInput::Notice(message) => SettingsInput::Notice(message),
        ModelsManagerInput::ConfirmDiscardPanel => {
            state.editor = Some(SettingsEditor::Confirm {
                title: "Discard unsaved changes?".to_string(),
                message:
                    "This form contains changes that have not been written to your user config."
                        .to_string(),
                confirm_label: "Discard changes".to_string(),
                action: SettingsConfirmAction::DiscardModelsPanel,
            });
            SettingsInput::Redraw
        }
        ModelsManagerInput::ConfirmConfig {
            title,
            message,
            confirm_label,
            write,
        } => {
            let (focus_provider, params) = write.into_parts();
            state.editor = Some(SettingsEditor::Confirm {
                title,
                message,
                confirm_label,
                action: SettingsConfirmAction::Write {
                    write: super::SettingsWrite {
                        focus: focus_provider
                            .map_or(SettingsFocus::Models, SettingsFocus::ModelsProvider),
                        params,
                    },
                    selected_theme: None,
                },
            });
            SettingsInput::Redraw
        }
        ModelsManagerInput::WriteConfig(write) => {
            let (focus_provider, params) = write.into_parts();
            SettingsInput::Write {
                write: super::SettingsWrite {
                    focus: focus_provider
                        .map_or(SettingsFocus::Models, SettingsFocus::ModelsProvider),
                    params,
                },
                selected_theme: None,
            }
        }
    }
}
