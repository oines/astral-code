use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

use crate::InputAction;
use crate::SurfaceActivity;
use crate::SurfaceState;

pub(super) fn handle_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    match (key.code, key.modifiers) {
        (KeyCode::Tab, KeyModifiers::NONE) | (KeyCode::Char('i'), KeyModifiers::NONE) => {
            state.focus_prompt();
            InputAction::Redraw
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
            state.move_entry_selection(1);
            InputAction::Redraw
        }
        (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
            state.move_entry_selection(-1);
            InputAction::Redraw
        }
        (KeyCode::Left, _) | (KeyCode::Char('h'), KeyModifiers::NONE) => {
            state.collapse_selected_entry();
            InputAction::Redraw
        }
        (KeyCode::Right, _) | (KeyCode::Char('l'), KeyModifiers::NONE) => {
            state.expand_selected_entry();
            InputAction::Redraw
        }
        (KeyCode::Char('e'), KeyModifiers::NONE) | (KeyCode::Enter, _) => {
            state.toggle_selected_entry();
            InputAction::Redraw
        }
        (KeyCode::PageUp, _) => InputAction::ScrollUp,
        (KeyCode::PageDown, _) => InputAction::ScrollDown,
        (KeyCode::BackTab, _) => InputAction::CycleMode,
        (KeyCode::Tab, modifiers) if modifiers.contains(KeyModifiers::SHIFT) => {
            InputAction::CycleMode
        }
        (KeyCode::Char('.'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            InputAction::OpenShortcuts
        }
        (KeyCode::Char('c'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            if matches!(state.activity(), SurfaceActivity::Working) {
                InputAction::Interrupt
            } else {
                InputAction::Exit
            }
        }
        _ => InputAction::None,
    }
}
