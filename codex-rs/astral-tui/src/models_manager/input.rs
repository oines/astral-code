use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;

use crate::modal::ModalPointerAction;

use super::ModelsConfigWrite;
use super::ModelsManagerState;
use super::SEARCH_ROW_ID;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ModelsManagerInput {
    None,
    Redraw,
    Cancel,
    Close,
    Notice(String),
    ConfirmDiscardPanel,
    ConfirmConfig {
        title: String,
        message: String,
        confirm_label: String,
        write: ModelsConfigWrite,
    },
    WriteConfig(ModelsConfigWrite),
}

pub(crate) fn handle_key(state: &mut ModelsManagerState, key: KeyEvent) -> ModelsManagerInput {
    if key.kind == KeyEventKind::Release {
        return ModelsManagerInput::None;
    }
    if key.kind == KeyEventKind::Repeat && matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')) {
        return ModelsManagerInput::None;
    }
    state.pointer.clear_hover();
    if key.code == KeyCode::Esc {
        if state.has_unsaved_form() {
            return ModelsManagerInput::ConfirmDiscardPanel;
        }
        return if state.close_panel() || state.search_focused() {
            state.focus_list();
            ModelsManagerInput::Redraw
        } else if state.clear_query() {
            ModelsManagerInput::Redraw
        } else {
            ModelsManagerInput::Cancel
        };
    }
    if state.capability_form_active() {
        return state.handle_capability_key(key);
    }
    if state.provider_form_active() {
        return state.handle_provider_key(key);
    }
    if state.detail.is_some() {
        return match (key.code, key.modifiers) {
            (KeyCode::Up | KeyCode::Char('k'), _) => {
                state.scroll_detail(-1);
                ModelsManagerInput::Redraw
            }
            (KeyCode::Down | KeyCode::Char('j'), _) => {
                state.scroll_detail(1);
                ModelsManagerInput::Redraw
            }
            (KeyCode::PageUp, _) => {
                state.scroll_detail(-10);
                ModelsManagerInput::Redraw
            }
            (KeyCode::PageDown, _) => {
                state.scroll_detail(10);
                ModelsManagerInput::Redraw
            }
            (KeyCode::Home | KeyCode::Char('g'), _) => {
                state.detail_to_start();
                ModelsManagerInput::Redraw
            }
            (KeyCode::End | KeyCode::Char('G'), _) => {
                state.detail_to_end();
                ModelsManagerInput::Redraw
            }
            (KeyCode::Enter, KeyModifiers::NONE) => state.activate_detail(),
            _ => ModelsManagerInput::None,
        };
    }
    if state.search_focused() {
        return match (key.code, key.modifiers) {
            (KeyCode::Up, _) => {
                state.move_search_selection(-1);
                ModelsManagerInput::Redraw
            }
            (KeyCode::Down, _) => {
                state.move_search_selection(1);
                ModelsManagerInput::Redraw
            }
            (KeyCode::PageUp, _) => {
                state.move_search_selection(-10);
                ModelsManagerInput::Redraw
            }
            (KeyCode::PageDown, _) => {
                state.move_search_selection(10);
                ModelsManagerInput::Redraw
            }
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                state.clear_query();
                ModelsManagerInput::Redraw
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                state.focus_list();
                ModelsManagerInput::Redraw
            }
            _ if state.edit_query(key) => ModelsManagerInput::Redraw,
            _ => ModelsManagerInput::None,
        };
    }
    match (key.code, key.modifiers) {
        (KeyCode::Up | KeyCode::Char('k'), _) => {
            state.move_selection(-1);
            ModelsManagerInput::Redraw
        }
        (KeyCode::Down | KeyCode::Char('j'), _) => {
            state.move_selection(1);
            ModelsManagerInput::Redraw
        }
        (KeyCode::PageUp, _) => {
            state.move_selection(-10);
            ModelsManagerInput::Redraw
        }
        (KeyCode::PageDown, _) => {
            state.move_selection(10);
            ModelsManagerInput::Redraw
        }
        (KeyCode::Home | KeyCode::Char('g'), _) => {
            state.select_start();
            ModelsManagerInput::Redraw
        }
        (KeyCode::End | KeyCode::Char('G'), _) => {
            state.select_end();
            ModelsManagerInput::Redraw
        }
        (KeyCode::Right | KeyCode::Char('l'), _) => {
            state.expand_selected();
            ModelsManagerInput::Redraw
        }
        (KeyCode::Left | KeyCode::Char('h'), _) => {
            state.collapse_selected();
            ModelsManagerInput::Redraw
        }
        (KeyCode::Enter, KeyModifiers::NONE) => state.activate(state.selected),
        (KeyCode::Char('/'), KeyModifiers::NONE) => {
            state.focus_search();
            ModelsManagerInput::Redraw
        }
        (KeyCode::Backspace, _) if !state.query_is_empty() => {
            state.focus_search();
            state.edit_query(key);
            ModelsManagerInput::Redraw
        }
        (KeyCode::Char(character), modifiers)
            if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT =>
        {
            state.focus_search();
            state.edit_query(KeyEvent::new(KeyCode::Char(character), modifiers));
            ModelsManagerInput::Redraw
        }
        _ => ModelsManagerInput::None,
    }
}

pub(crate) fn handle_paste(state: &mut ModelsManagerState, text: &str) -> ModelsManagerInput {
    if state.capability_form_active() {
        return state.handle_capability_paste(text);
    }
    if state.provider_form_active() {
        return state.handle_provider_paste(text);
    }
    if state.detail.is_some() {
        return ModelsManagerInput::None;
    }
    state.focus_search();
    state.paste_query(text);
    ModelsManagerInput::Redraw
}

pub(crate) fn handle_mouse(
    state: &mut ModelsManagerState,
    mouse: MouseEvent,
) -> ModelsManagerInput {
    let position = (mouse.column, mouse.row);
    match state.pointer.handle_mouse(mouse) {
        ModalPointerAction::Ignored => ModelsManagerInput::None,
        ModalPointerAction::Close => {
            if state.has_unsaved_form() {
                ModelsManagerInput::ConfirmDiscardPanel
            } else if state.close_panel() {
                ModelsManagerInput::Redraw
            } else {
                ModelsManagerInput::Close
            }
        }
        ModalPointerAction::Redraw | ModalPointerAction::Hover(None) => ModelsManagerInput::Redraw,
        ModalPointerAction::Hover(Some(_)) => ModelsManagerInput::Redraw,
        ModalPointerAction::Activate(index) => {
            if index == SEARCH_ROW_ID
                && !state.capability_form_active()
                && !state.provider_form_active()
                && state.detail.is_none()
            {
                state.focus_search();
                ModelsManagerInput::Redraw
            } else if state.capability_form_active() {
                state.activate_capability_field(index)
            } else if state.provider_form_active() {
                state.activate_provider_field(index)
            } else if state.detail.is_some() {
                state.activate_detail()
            } else {
                if state.provider_toggle_hit(index, position.0, position.1) {
                    state.set_selected(index);
                    return state.activate(index);
                }
                if state.selected != index {
                    state.set_selected(index);
                    ModelsManagerInput::Redraw
                } else {
                    state.activate(index)
                }
            }
        }
        ModalPointerAction::Scroll(delta) => {
            state.pointer.clear_hover();
            if state.capability_form_active() {
                state.move_capability_field(delta);
            } else if state.provider_form_active() {
                state.move_provider_field(delta);
            } else if state.detail.is_some() {
                state.scroll_detail(delta);
            } else {
                state.scroll_browser(delta);
            }
            ModelsManagerInput::Redraw
        }
    }
}
