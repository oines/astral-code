use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

use crate::InputAction;
use crate::SurfaceState;

pub(super) fn handle_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    match key {
        KeyEvent {
            code: KeyCode::Esc | KeyCode::Tab,
            modifiers: KeyModifiers::NONE,
            ..
        } => {
            state.blur_queue();
            InputAction::Redraw
        }
        KeyEvent {
            code: KeyCode::Up | KeyCode::Char('k'),
            modifiers: KeyModifiers::NONE,
            ..
        }
        | KeyEvent {
            code: KeyCode::Char('p'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => {
            state.move_queue_selection(-1);
            InputAction::Redraw
        }
        KeyEvent {
            code: KeyCode::Down | KeyCode::Char('j'),
            modifiers: KeyModifiers::NONE,
            ..
        }
        | KeyEvent {
            code: KeyCode::Char('n'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => {
            state.move_queue_selection(1);
            InputAction::Redraw
        }
        KeyEvent {
            code: KeyCode::Char('e') | KeyCode::Enter,
            modifiers: KeyModifiers::NONE,
            ..
        } => {
            if state.begin_queue_edit() {
                InputAction::Redraw
            } else {
                InputAction::None
            }
        }
        KeyEvent {
            code: KeyCode::Char('x') | KeyCode::Delete | KeyCode::Backspace,
            modifiers: KeyModifiers::NONE,
            ..
        } => {
            if state.remove_selected_follow_up() {
                InputAction::DrainQueue
            } else {
                InputAction::None
            }
        }
        KeyEvent {
            code: KeyCode::Char('J'),
            modifiers,
            ..
        } if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT => {
            state.reorder_selected_follow_up(1);
            InputAction::Redraw
        }
        KeyEvent {
            code: KeyCode::Char('K'),
            modifiers,
            ..
        } if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT => {
            state.reorder_selected_follow_up(-1);
            InputAction::Redraw
        }
        KeyEvent {
            code: KeyCode::Char('y'),
            modifiers: KeyModifiers::NONE,
            ..
        } => state
            .selected_follow_up_text()
            .map_or(InputAction::None, |text| InputAction::CopyText {
                text: text.to_string(),
                notice: "Copied queued follow-up".to_string(),
            }),
        _ => InputAction::None,
    }
}
