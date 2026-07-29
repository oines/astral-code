use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

use crate::InputAction;
use crate::SurfaceState;

pub(super) fn handle_key(state: &mut SurfaceState, key: KeyEvent) -> Option<InputAction> {
    match (key.code, key.modifiers) {
        (KeyCode::Up, _) | (KeyCode::Char('p' | 'k'), KeyModifiers::CONTROL) => {
            state.move_file_search_selection(-1);
            Some(InputAction::Redraw)
        }
        (KeyCode::Down, _) | (KeyCode::Char('n' | 'j'), KeyModifiers::CONTROL) => {
            state.move_file_search_selection(1);
            Some(InputAction::Redraw)
        }
        (KeyCode::PageUp, _) | (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
            state.page_file_search_selection(-1);
            Some(InputAction::Redraw)
        }
        (KeyCode::PageDown, _) | (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
            state.page_file_search_selection(1);
            Some(InputAction::Redraw)
        }
        (KeyCode::Esc, _) => {
            state.dismiss_file_search();
            Some(InputAction::Redraw)
        }
        (KeyCode::Char(':'), KeyModifiers::NONE) | (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
            state
                .open_file_search_viewer()
                .then_some(InputAction::Redraw)
        }
        (KeyCode::Tab | KeyCode::Enter, KeyModifiers::NONE) => state
            .accept_file_search_selection()
            .then_some(InputAction::Redraw),
        (KeyCode::Right, KeyModifiers::NONE) => state
            .drill_into_file_search_selection()
            .then_some(InputAction::Redraw),
        _ => None,
    }
}
