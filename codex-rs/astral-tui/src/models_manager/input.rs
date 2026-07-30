use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;

use crate::modal::ModalPointerAction;

use super::ModelsManagerState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModelsManagerInput {
    None,
    Redraw,
    Cancel,
}

pub(crate) fn handle_key(state: &mut ModelsManagerState, key: KeyEvent) -> ModelsManagerInput {
    if key.kind == KeyEventKind::Release {
        return ModelsManagerInput::None;
    }
    if key.code == KeyCode::Esc {
        return if state.close_detail() {
            ModelsManagerInput::Redraw
        } else {
            ModelsManagerInput::Cancel
        };
    }
    if state.detail.is_some() {
        return ModelsManagerInput::None;
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
            state.set_selected(0);
            ModelsManagerInput::Redraw
        }
        (KeyCode::End, _) => {
            state.set_selected(state.rows().len().saturating_sub(1));
            ModelsManagerInput::Redraw
        }
        (KeyCode::Enter, KeyModifiers::NONE) => state.activate(state.selected),
        (KeyCode::Backspace, _) => {
            state.query.pop();
            state.clamp_selection();
            ModelsManagerInput::Redraw
        }
        (KeyCode::Char(character), modifiers)
            if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT =>
        {
            state.query.push(character);
            state.clamp_selection();
            ModelsManagerInput::Redraw
        }
        _ => ModelsManagerInput::None,
    }
}

pub(crate) fn handle_paste(state: &mut ModelsManagerState, text: &str) -> ModelsManagerInput {
    if state.detail.is_some() {
        return ModelsManagerInput::None;
    }
    state
        .query
        .extend(text.chars().filter(|character| !character.is_control()));
    state.clamp_selection();
    ModelsManagerInput::Redraw
}

pub(crate) fn handle_mouse(
    state: &mut ModelsManagerState,
    mouse: MouseEvent,
) -> ModelsManagerInput {
    match state.pointer.handle_mouse(mouse) {
        ModalPointerAction::Ignored => ModelsManagerInput::None,
        ModalPointerAction::Close => {
            if state.close_detail() {
                ModelsManagerInput::Redraw
            } else {
                ModelsManagerInput::Cancel
            }
        }
        ModalPointerAction::Redraw | ModalPointerAction::Hover(None) => ModelsManagerInput::Redraw,
        ModalPointerAction::Hover(Some(index)) => {
            state.set_selected(index);
            ModelsManagerInput::Redraw
        }
        ModalPointerAction::Activate(index) => {
            state.set_selected(index);
            state.activate(index)
        }
        ModalPointerAction::Scroll(delta) => {
            state.move_selection(delta);
            ModelsManagerInput::Redraw
        }
    }
}
