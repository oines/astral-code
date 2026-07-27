use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

use crate::InputAction;
use crate::SurfaceState;

pub(super) fn handle_key(state: &mut SurfaceState, key: KeyEvent) -> Option<InputAction> {
    match (key.code, key.modifiers) {
        (KeyCode::Up, _) | (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
            state.move_mention_selection(-1);
            Some(InputAction::Redraw)
        }
        (KeyCode::Down, _) | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
            state.move_mention_selection(1);
            Some(InputAction::Redraw)
        }
        (KeyCode::Esc, _) => {
            state.dismiss_mentions();
            Some(InputAction::Redraw)
        }
        (KeyCode::Tab, _) => {
            if !state.accept_mention_selection() {
                state.dismiss_mentions();
            }
            Some(InputAction::Redraw)
        }
        (KeyCode::Enter, KeyModifiers::NONE) => {
            if state.accept_mention_selection() {
                Some(InputAction::Redraw)
            } else {
                state.dismiss_mentions();
                None
            }
        }
        _ => None,
    }
}
