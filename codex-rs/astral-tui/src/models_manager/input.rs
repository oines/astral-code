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
    WriteConfig(ModelsConfigWrite),
}

pub(crate) fn handle_key(state: &mut ModelsManagerState, key: KeyEvent) -> ModelsManagerInput {
    if key.kind == KeyEventKind::Release {
        return ModelsManagerInput::None;
    }
    state.pointer.clear_hover();
    if key.code == KeyCode::Esc {
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
        return if key.code == KeyCode::Enter && key.modifiers == KeyModifiers::NONE {
            state.activate_detail()
        } else {
            ModelsManagerInput::None
        };
    }
    if state.search_focused() {
        return match (key.code, key.modifiers) {
            (KeyCode::Up, _) => {
                state.select_end();
                ModelsManagerInput::Redraw
            }
            (KeyCode::Down, _) => {
                state.select_start();
                ModelsManagerInput::Redraw
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                state.focus_list();
                state.activate(state.selected)
            }
            _ if state.edit_query(key) => ModelsManagerInput::Redraw,
            _ => ModelsManagerInput::None,
        };
    }
    match (key.code, key.modifiers) {
        (KeyCode::Up, _) => {
            state.move_selection(-1);
            ModelsManagerInput::Redraw
        }
        (KeyCode::Down, _) => {
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
        (KeyCode::Home, _) => {
            state.select_start();
            ModelsManagerInput::Redraw
        }
        (KeyCode::End, _) => {
            state.select_end();
            ModelsManagerInput::Redraw
        }
        (KeyCode::Right, _) => {
            state.expand_selected();
            ModelsManagerInput::Redraw
        }
        (KeyCode::Left, _) => {
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
    match state.pointer.handle_mouse(mouse) {
        ModalPointerAction::Ignored => ModelsManagerInput::None,
        ModalPointerAction::Close => {
            if state.close_panel() {
                ModelsManagerInput::Redraw
            } else {
                ModelsManagerInput::Cancel
            }
        }
        ModalPointerAction::Redraw | ModalPointerAction::Hover(None) => ModelsManagerInput::Redraw,
        ModalPointerAction::Hover(Some(index)) => {
            if index != SEARCH_ROW_ID
                && !state.capability_form_active()
                && !state.provider_form_active()
                && state.detail.is_none()
            {
                state.set_selected(index);
            }
            ModelsManagerInput::Redraw
        }
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
                state.set_selected(index);
                state.activate(index)
            }
        }
        ModalPointerAction::Scroll(delta) => {
            state.pointer.clear_hover();
            if state.capability_form_active() {
                state.move_capability_field(delta);
            } else if state.provider_form_active() {
                state.move_provider_field(delta);
            } else if state.detail.is_none() {
                state.scroll_browser(delta);
            }
            ModelsManagerInput::Redraw
        }
    }
}
