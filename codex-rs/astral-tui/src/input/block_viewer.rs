use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;

use crate::InputAction;
use crate::SurfaceState;
use crate::block_viewer::BlockViewerMouseAction;
use crate::block_viewer::BlockViewerState;

pub(super) fn handle_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    if key.code == KeyCode::Char('f') && key.modifiers.contains(KeyModifiers::CONTROL)
        || !state
            .block_viewer()
            .is_some_and(BlockViewerState::search_input_active)
            && (key.code == KeyCode::Esc
                || key.code == KeyCode::Char('q') && key.modifiers == KeyModifiers::NONE)
    {
        state.close_block_viewer();
        return InputAction::Redraw;
    }
    if state
        .block_viewer()
        .is_some_and(BlockViewerState::search_input_active)
    {
        if let Some(viewer) = state.block_viewer_mut() {
            viewer.handle_search_key(key);
        }
        return InputAction::Redraw;
    }
    match (key.code, key.modifiers) {
        (KeyCode::Char('/'), KeyModifiers::NONE) => {
            if let Some(viewer) = state.block_viewer_mut() {
                viewer.open_search();
            }
            return InputAction::Redraw;
        }
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
    match (key.code, key.modifiers) {
        (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
            viewer.select_by(-1);
            InputAction::Redraw
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
            viewer.select_by(1);
            InputAction::Redraw
        }
        (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
            viewer.select_by(-1);
            InputAction::Redraw
        }
        (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
            viewer.select_by(1);
            InputAction::Redraw
        }
        (KeyCode::Char('n'), KeyModifiers::NONE) => {
            viewer.select_next_match();
            InputAction::Redraw
        }
        (KeyCode::Char('N'), KeyModifiers::SHIFT) => {
            viewer.select_previous_match();
            InputAction::Redraw
        }
        (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
            viewer.scroll_by(-1);
            InputAction::Redraw
        }
        (KeyCode::Char('j'), KeyModifiers::CONTROL) => {
            viewer.scroll_by(1);
            InputAction::Redraw
        }
        (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
            viewer.scroll_half_page(-1);
            InputAction::Redraw
        }
        (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
            viewer.scroll_half_page(1);
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

pub(super) fn handle_paste(state: &mut SurfaceState, text: &str) -> InputAction {
    let Some(viewer) = state.block_viewer_mut() else {
        return InputAction::None;
    };
    if viewer.search_input_active() {
        viewer.handle_search_paste(text);
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
    }
}
