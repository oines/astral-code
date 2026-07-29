use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;

use crate::InputAction;
use crate::SurfaceState;
use crate::block_viewer::BlockViewerMouseAction;

use super::content_viewer;
use super::content_viewer::ViewerKeyResult;

pub(super) fn handle_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    if key.code == KeyCode::Char('f') && key.modifiers.contains(KeyModifiers::CONTROL) {
        state.close_block_viewer();
        return InputAction::Redraw;
    }
    let query_input_active = state
        .block_viewer()
        .is_some_and(|viewer| viewer.query_input_active());
    if query_input_active {
        let Some(viewer) = state.block_viewer_mut() else {
            return InputAction::None;
        };
        content_viewer::handle_key(viewer, key);
        return InputAction::Redraw;
    }
    if let Some(viewer) = state.block_viewer_mut() {
        viewer.clear_text_drag();
    }
    match (key.code, key.modifiers) {
        (KeyCode::Char('r'), KeyModifiers::NONE) => {
            return if state.toggle_block_viewer_raw() {
                InputAction::Redraw
            } else {
                InputAction::None
            };
        }
        (KeyCode::Char('y'), KeyModifiers::NONE) => {
            return state
                .block_viewer_copy_text()
                .map_or(InputAction::None, |text| InputAction::CopyText {
                    text,
                    notice: "Copied block content".to_string(),
                });
        }
        (KeyCode::Char('Y'), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            return state
                .block_viewer_copy_meta()
                .map_or(InputAction::None, |text| InputAction::CopyText {
                    text,
                    notice: "Copied block metadata".to_string(),
                });
        }
        _ => {}
    }
    let Some(viewer) = state.block_viewer_mut() else {
        return InputAction::None;
    };
    match content_viewer::handle_key(viewer, key) {
        ViewerKeyResult::Handled => InputAction::Redraw,
        ViewerKeyResult::Close => {
            state.close_block_viewer();
            InputAction::Redraw
        }
        ViewerKeyResult::Unhandled => InputAction::None,
    }
}

pub(super) fn handle_paste(state: &mut SurfaceState, text: &str) -> InputAction {
    let Some(viewer) = state.block_viewer_mut() else {
        return InputAction::None;
    };
    if viewer.query_input_active() {
        viewer.clear_text_drag();
        viewer.handle_query_paste(text);
        InputAction::Redraw
    } else {
        InputAction::None
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
        BlockViewerMouseAction::Copy(text) => InputAction::CopyText {
            text,
            notice: "Copied selection".to_string(),
        },
    }
}
