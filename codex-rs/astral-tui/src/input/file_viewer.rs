use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;

use crate::InputAction;
use crate::SurfaceState;
use crate::block_viewer::ViewerMouseAction;

use super::content_viewer;
use super::content_viewer::ViewerKeyResult;

pub(super) fn handle_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    let query_input_active = state
        .file_viewer()
        .is_some_and(|viewer| viewer.viewer().query_input_active());
    if query_input_active {
        let Some(viewer) = state.file_viewer_mut() else {
            return InputAction::None;
        };
        content_viewer::handle_key(viewer.viewer_mut(), key);
        return InputAction::Redraw;
    }
    match (key.code, key.modifiers) {
        (KeyCode::Enter, KeyModifiers::NONE) => {
            let include_range = state
                .file_viewer()
                .is_some_and(|viewer| viewer.viewer().visual_selection_active());
            return if state.confirm_file_viewer(include_range) {
                InputAction::Redraw
            } else {
                InputAction::None
            };
        }
        (KeyCode::Char('x'), KeyModifiers::NONE) => {
            return if state.confirm_file_viewer(false) {
                InputAction::Redraw
            } else {
                InputAction::None
            };
        }
        (KeyCode::Char('y'), KeyModifiers::NONE) => {
            return state
                .file_viewer_copy_text()
                .map_or(InputAction::None, |text| InputAction::CopyText {
                    text,
                    notice: "Copied selected lines".to_string(),
                });
        }
        (KeyCode::Char('Y'), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            return state
                .file_viewer_copy_path()
                .map_or(InputAction::None, |text| InputAction::CopyText {
                    text,
                    notice: "Copied file path".to_string(),
                });
        }
        _ => {}
    }
    let Some(viewer) = state.file_viewer_mut() else {
        return InputAction::None;
    };
    match content_viewer::handle_key(viewer.viewer_mut(), key) {
        ViewerKeyResult::Handled => InputAction::Redraw,
        ViewerKeyResult::Close => {
            state.close_file_viewer();
            InputAction::Redraw
        }
        ViewerKeyResult::Unhandled => InputAction::None,
    }
}

pub(super) fn handle_paste(state: &mut SurfaceState, text: &str) -> InputAction {
    let Some(viewer) = state.file_viewer_mut() else {
        return InputAction::None;
    };
    if viewer.viewer().query_input_active() {
        viewer.viewer_mut().clear_text_drag();
        viewer.viewer_mut().handle_query_paste(text);
        InputAction::Redraw
    } else {
        InputAction::None
    }
}

pub(super) fn handle_mouse(state: &mut SurfaceState, mouse: MouseEvent) -> InputAction {
    let Some(viewer) = state.file_viewer_mut() else {
        return InputAction::None;
    };
    match viewer.viewer_mut().handle_mouse(mouse) {
        ViewerMouseAction::Ignored | ViewerMouseAction::Redraw => InputAction::Redraw,
        ViewerMouseAction::Close => {
            state.close_file_viewer();
            InputAction::Redraw
        }
        ViewerMouseAction::Copy(text) => InputAction::CopyText {
            text,
            notice: "Copied selection".to_string(),
        },
    }
}
