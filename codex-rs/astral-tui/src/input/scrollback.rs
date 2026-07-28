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
        (KeyCode::Char(' '), KeyModifiers::NONE) => {
            state.focus_prompt();
            InputAction::Redraw
        }
        (KeyCode::Char('H'), KeyModifiers::NONE | KeyModifiers::SHIFT)
        | (KeyCode::Left, KeyModifiers::SHIFT) => {
            state.previous_turn();
            InputAction::Redraw
        }
        (KeyCode::Char('L'), KeyModifiers::NONE | KeyModifiers::SHIFT)
        | (KeyCode::Right, KeyModifiers::SHIFT) => {
            state.next_turn();
            InputAction::Redraw
        }
        (KeyCode::Char('J'), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            state.next_response();
            InputAction::Redraw
        }
        (KeyCode::Char('K'), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            state.previous_response();
            InputAction::Redraw
        }
        (KeyCode::Char('g'), KeyModifiers::NONE) => {
            state.goto_scrollback_top();
            InputAction::Redraw
        }
        (KeyCode::Char('G'), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            state.goto_scrollback_bottom();
            InputAction::Redraw
        }
        (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
            state.scroll_up(/* lines */ 1);
            InputAction::Redraw
        }
        (KeyCode::Char('j'), KeyModifiers::CONTROL) => {
            state.scroll_down(/* lines */ 1);
            InputAction::Redraw
        }
        (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
            state.half_page_up();
            InputAction::Redraw
        }
        (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
            state.half_page_down();
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
        (KeyCode::Char('e'), KeyModifiers::NONE) => {
            state.toggle_selected_entry();
            InputAction::Redraw
        }
        (KeyCode::Char('E'), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            state.toggle_all_entries();
            InputAction::Redraw
        }
        (KeyCode::Char('e'), modifiers) if modifiers == KeyModifiers::CONTROL => {
            state.toggle_all_thinking();
            InputAction::Redraw
        }
        (KeyCode::Char('r'), KeyModifiers::NONE) => {
            if state.toggle_selected_raw() {
                InputAction::Redraw
            } else {
                InputAction::None
            }
        }
        (KeyCode::Char('y'), KeyModifiers::NONE) => {
            state
                .selected_copy_text()
                .map_or(InputAction::None, |text| InputAction::CopyText {
                    text,
                    notice: "Copied block content".to_string(),
                })
        }
        (KeyCode::Char('Y'), KeyModifiers::NONE | KeyModifiers::SHIFT) => state
            .selected_copy_meta()
            .map_or(InputAction::None, |text| InputAction::CopyText {
                text,
                notice: "Copied block metadata".to_string(),
            }),
        (KeyCode::Enter, KeyModifiers::NONE) => {
            if state.open_selected_entry() {
                InputAction::Redraw
            } else {
                InputAction::None
            }
        }
        (KeyCode::Char('f'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            if state.open_selected_entry() {
                InputAction::Redraw
            } else {
                InputAction::None
            }
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
