use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;

use crate::InputAction;
use crate::SurfaceState;
use crate::modal::ModalPointerAction;

pub(super) fn handle_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    if is_shortcuts_key(key) {
        state.close_shortcut_help();
        return InputAction::Redraw;
    }
    let Some(help) = state.shortcut_help_mut() else {
        return InputAction::None;
    };
    if help.detail().is_some() {
        return match key.code {
            KeyCode::Esc | KeyCode::Left | KeyCode::Backspace => {
                help.close_detail();
                InputAction::Redraw
            }
            KeyCode::Up | KeyCode::PageUp => {
                help.detail_scroll = help.detail_scroll.saturating_sub(1);
                InputAction::Redraw
            }
            KeyCode::Down | KeyCode::PageDown => {
                help.detail_scroll = help.detail_scroll.saturating_add(1);
                InputAction::Redraw
            }
            KeyCode::Home => {
                help.detail_scroll = 0;
                InputAction::Redraw
            }
            _ => InputAction::None,
        };
    }
    if help.search_active() {
        return match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                help.clear_search();
                InputAction::Redraw
            }
            (KeyCode::Up, _) => {
                help.move_selection(-1);
                InputAction::Redraw
            }
            (KeyCode::Down, _) => {
                help.move_selection(1);
                InputAction::Redraw
            }
            (KeyCode::Enter, _) => {
                help.open_selected_detail();
                InputAction::Redraw
            }
            (KeyCode::Backspace, _) => {
                help.backspace_query();
                InputAction::Redraw
            }
            (KeyCode::Char(character), modifiers)
                if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT =>
            {
                help.insert_query(character);
                InputAction::Redraw
            }
            _ => InputAction::None,
        };
    }
    match (key.code, key.modifiers) {
        (KeyCode::Esc, _) => {
            state.close_shortcut_help();
            InputAction::Redraw
        }
        (KeyCode::Char('/' | 'i'), KeyModifiers::NONE) => {
            help.begin_search();
            InputAction::Redraw
        }
        (KeyCode::Char('f'), KeyModifiers::NONE) => {
            help.toggle_filter();
            InputAction::Redraw
        }
        (KeyCode::Up | KeyCode::Char('k'), KeyModifiers::NONE) => {
            help.move_selection(-1);
            InputAction::Redraw
        }
        (KeyCode::Down | KeyCode::Char('j'), KeyModifiers::NONE) => {
            help.move_selection(1);
            InputAction::Redraw
        }
        (KeyCode::PageUp, _) => {
            help.move_selection(-10);
            InputAction::Redraw
        }
        (KeyCode::PageDown, _) => {
            help.move_selection(10);
            InputAction::Redraw
        }
        (KeyCode::Home | KeyCode::Char('g'), KeyModifiers::NONE) => {
            help.select_start();
            InputAction::Redraw
        }
        (KeyCode::End, _) | (KeyCode::Char('G'), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            help.select_end();
            InputAction::Redraw
        }
        (KeyCode::Char('e' | ' '), KeyModifiers::NONE) => {
            help.toggle_selected();
            InputAction::Redraw
        }
        (KeyCode::Left | KeyCode::Char('h'), KeyModifiers::NONE) => {
            help.collapse_selected();
            InputAction::Redraw
        }
        (KeyCode::Right | KeyCode::Char('l'), KeyModifiers::NONE) => {
            help.expand_selected();
            InputAction::Redraw
        }
        (KeyCode::Enter, KeyModifiers::NONE) => {
            help.open_selected_detail();
            InputAction::Redraw
        }
        _ => InputAction::None,
    }
}

pub(super) fn handle_paste(state: &mut SurfaceState, text: &str) -> InputAction {
    let Some(help) = state.shortcut_help_mut() else {
        return InputAction::None;
    };
    if help.detail().is_some() {
        return InputAction::None;
    }
    help.begin_search();
    for character in text.chars().filter(|character| !character.is_control()) {
        help.insert_query(character);
    }
    InputAction::Redraw
}

pub(super) fn handle_mouse(state: &mut SurfaceState, mouse: MouseEvent) -> InputAction {
    let action = state
        .shortcut_help_mut()
        .map(|help| help.pointer.handle_mouse(mouse))
        .unwrap_or(ModalPointerAction::Ignored);
    match action {
        ModalPointerAction::Ignored => InputAction::None,
        ModalPointerAction::Close => {
            state.close_shortcut_help();
            InputAction::Redraw
        }
        ModalPointerAction::Scroll(delta) => {
            if let Some(help) = state.shortcut_help_mut() {
                if help.detail().is_some() {
                    help.detail_scroll = help.detail_scroll.saturating_add_signed(delta);
                } else {
                    help.move_selection(delta);
                }
            }
            InputAction::Redraw
        }
        ModalPointerAction::Hover(_) | ModalPointerAction::Redraw => InputAction::Redraw,
        ModalPointerAction::Activate(row) => {
            let Some(help) = state.shortcut_help_mut() else {
                return InputAction::None;
            };
            if help.selected() == row {
                help.open_selected_detail();
            } else {
                help.select(row);
            }
            InputAction::Redraw
        }
    }
}

fn is_shortcuts_key(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('.' | 'x')) && key.modifiers.contains(KeyModifiers::CONTROL)
}
