use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;

use crate::InputAction;
use crate::SurfaceState;
use crate::block_viewer::BlockViewerMouseAction;
use crate::block_viewer::BlockViewerState;

pub(super) fn handle_key(state: &mut SurfaceState, key: KeyEvent) -> InputAction {
    if key.code == KeyCode::Char('f') && key.modifiers.contains(KeyModifiers::CONTROL) {
        state.close_block_viewer();
        return InputAction::Redraw;
    }
    if let Some(viewer) = state.block_viewer_mut() {
        viewer.clear_text_drag();
    }
    if state
        .block_viewer()
        .is_some_and(BlockViewerState::query_input_active)
    {
        if let Some(viewer) = state.block_viewer_mut() {
            viewer.handle_query_key(key);
        }
        return InputAction::Redraw;
    }
    if key.code == KeyCode::Esc {
        let Some(viewer) = state.block_viewer_mut() else {
            return InputAction::None;
        };
        if viewer.clear_visual_selection() || viewer.clear_matcher() {
            return InputAction::Redraw;
        }
        state.close_block_viewer();
        return InputAction::Redraw;
    }
    if key.code == KeyCode::Char('q')
        && key.modifiers == KeyModifiers::NONE
        && !state
            .block_viewer()
            .is_some_and(BlockViewerState::visual_selection_active)
    {
        state.close_block_viewer();
        return InputAction::Redraw;
    }
    match (key.code, key.modifiers) {
        (KeyCode::Char('/'), KeyModifiers::NONE) => {
            if let Some(viewer) = state.block_viewer_mut() {
                viewer.open_search();
            }
            return InputAction::Redraw;
        }
        (KeyCode::Char('f'), KeyModifiers::NONE) => {
            if let Some(viewer) = state.block_viewer_mut() {
                viewer.open_filter();
            }
            return InputAction::Redraw;
        }
        (KeyCode::Char('w'), KeyModifiers::NONE) => {
            if let Some(viewer) = state.block_viewer_mut() {
                viewer.toggle_wrap_mode();
            }
            return InputAction::Redraw;
        }
        (KeyCode::Char('v' | 'V'), modifiers)
            if modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT =>
        {
            if let Some(viewer) = state.block_viewer_mut() {
                viewer.toggle_visual_selection();
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
    if matches!(
        (key.code, key.modifiers),
        (KeyCode::Char('J'), KeyModifiers::SHIFT) | (KeyCode::Down, KeyModifiers::SHIFT)
    ) {
        viewer.begin_visual_selection();
        viewer.select_by(1);
        return InputAction::Redraw;
    }
    if matches!(
        (key.code, key.modifiers),
        (KeyCode::Char('K'), KeyModifiers::SHIFT) | (KeyCode::Up, KeyModifiers::SHIFT)
    ) {
        viewer.begin_visual_selection();
        viewer.select_by(-1);
        return InputAction::Redraw;
    }
    if matches!(
        (key.code, key.modifiers),
        (KeyCode::PageDown, KeyModifiers::SHIFT)
            | (
                KeyCode::Char('d' | 'D'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            )
    ) {
        viewer.begin_visual_selection();
        viewer.scroll_page(1);
        return InputAction::Redraw;
    }
    if matches!(
        (key.code, key.modifiers),
        (KeyCode::PageUp, KeyModifiers::SHIFT)
            | (
                KeyCode::Char('u' | 'U'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            )
    ) {
        viewer.begin_visual_selection();
        viewer.scroll_page(-1);
        return InputAction::Redraw;
    }
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
