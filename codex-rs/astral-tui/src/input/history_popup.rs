use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

use crate::InputAction;
use crate::SurfaceState;

pub(super) fn handle_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    if key.code == KeyCode::Esc
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
    {
        state.cancel_history();
        return InputAction::Redraw;
    }
    if matches!(key.code, KeyCode::Enter | KeyCode::Tab) {
        state.accept_history_selection();
        return InputAction::Redraw;
    }
    if key.code == KeyCode::Up
        || (key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('p' | 'k')))
    {
        state.move_history_selection(-1);
        return InputAction::Redraw;
    }
    if key.code == KeyCode::Down
        || (key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('n' | 'j')))
    {
        if !state.move_history_selection(1) {
            state.cancel_history();
        }
        return InputAction::Redraw;
    }
    if key.code == KeyCode::PageUp
        || (key.code == KeyCode::Char('u') && key.modifiers.contains(KeyModifiers::CONTROL))
    {
        state.page_history_selection(-1);
        return InputAction::Redraw;
    }
    if key.code == KeyCode::PageDown
        || (key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL))
    {
        state.page_history_selection(1);
        return InputAction::Redraw;
    }

    let browse = state.history().browse;
    if browse {
        state.detach_history();
    }
    if state.composer_state_mut().edit_key(key) {
        if browse {
            state.refresh_composer_completions();
        } else {
            state.update_history_query();
        }
        InputAction::Redraw
    } else {
        InputAction::None
    }
}

pub(super) fn handle_paste(state: &mut SurfaceState, text: &str) -> InputAction {
    let browse = state.history().browse;
    if browse {
        state.detach_history();
    }
    state.composer_state_mut().insert_text(text);
    if browse {
        state.refresh_composer_completions();
    } else {
        state.update_history_query();
    }
    InputAction::Redraw
}
