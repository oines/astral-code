use crossterm::event::KeyEvent;
use crossterm::event::MouseEvent;

use super::InputAction;
use crate::SurfaceState;
use crate::models_manager::ModelsManagerInput;

pub(super) fn handle_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    let input = state
        .models_manager_mut()
        .map(|manager| crate::models_manager::handle_key(manager, key))
        .unwrap_or(ModelsManagerInput::None);
    apply_input(state, input)
}

pub(super) fn handle_paste(state: &mut SurfaceState, text: &str) -> InputAction {
    let input = state
        .models_manager_mut()
        .map(|manager| crate::models_manager::handle_paste(manager, text))
        .unwrap_or(ModelsManagerInput::None);
    apply_input(state, input)
}

pub(super) fn handle_mouse(state: &mut SurfaceState, mouse: MouseEvent) -> InputAction {
    let input = state
        .models_manager_mut()
        .map(|manager| crate::models_manager::handle_mouse(manager, mouse))
        .unwrap_or(ModelsManagerInput::None);
    apply_input(state, input)
}

fn apply_input(state: &mut SurfaceState, input: ModelsManagerInput) -> InputAction {
    match input {
        ModelsManagerInput::None => InputAction::None,
        ModelsManagerInput::Redraw => InputAction::Redraw,
        ModelsManagerInput::Cancel => {
            state.close_models_manager();
            InputAction::Redraw
        }
    }
}
