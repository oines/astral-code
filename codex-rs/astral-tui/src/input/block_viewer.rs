use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;

use crate::InputAction;
use crate::SurfaceState;
use crate::block_viewer::BlockViewerMouseAction;

pub(super) fn handle_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    if key.code == KeyCode::Esc
        || key.code == KeyCode::Char('q') && key.modifiers == KeyModifiers::NONE
        || key.code == KeyCode::Char('f') && key.modifiers.contains(KeyModifiers::CONTROL)
    {
        state.close_block_viewer();
        return InputAction::Redraw;
    }
    let Some(viewer) = state.block_viewer_mut() else {
        return InputAction::None;
    };
    match (key.code, key.modifiers) {
        (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
            viewer.scroll_by(-1);
            InputAction::Redraw
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
            viewer.scroll_by(1);
            InputAction::Redraw
        }
        (KeyCode::PageUp, _) => {
            viewer.scroll_page(-1);
            InputAction::Redraw
        }
        (KeyCode::PageDown, _) => {
            viewer.scroll_page(1);
            InputAction::Redraw
        }
        (KeyCode::Home, _) | (KeyCode::Char('g'), KeyModifiers::NONE) => {
            viewer.scroll_to_start();
            InputAction::Redraw
        }
        (KeyCode::End, _) | (KeyCode::Char('G'), KeyModifiers::SHIFT) => {
            viewer.scroll_to_end();
            InputAction::Redraw
        }
        _ => InputAction::None,
    }
}

pub(super) fn handle_mouse(state: &mut SurfaceState, mouse: MouseEvent) -> InputAction {
    let Some(viewer) = state.block_viewer_mut() else {
        return InputAction::None;
    };
    match viewer.handle_mouse(mouse) {
        BlockViewerMouseAction::Ignored | BlockViewerMouseAction::Redraw => InputAction::Redraw,
        BlockViewerMouseAction::Close => {
            state.close_block_viewer();
            InputAction::Redraw
        }
    }
}
