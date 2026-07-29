//! Shared keyboard behavior for transcript and file content viewers.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

use crate::block_viewer::ViewerState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ViewerKeyResult {
    Handled,
    Close,
    Unhandled,
}

pub(super) fn handle_key(viewer: &mut ViewerState, key: KeyEvent) -> ViewerKeyResult {
    viewer.clear_text_drag();
    if viewer.query_input_active() {
        viewer.handle_query_key(key);
        return ViewerKeyResult::Handled;
    }
    if key.code == KeyCode::Esc {
        return if viewer.clear_visual_selection() || viewer.clear_matcher() {
            ViewerKeyResult::Handled
        } else {
            ViewerKeyResult::Close
        };
    }
    if key.code == KeyCode::Char('q')
        && key.modifiers == KeyModifiers::NONE
        && !viewer.visual_selection_active()
    {
        return ViewerKeyResult::Close;
    }
    match (key.code, key.modifiers) {
        (KeyCode::Char('/'), KeyModifiers::NONE) => viewer.open_search(),
        (KeyCode::Char('f'), KeyModifiers::NONE) => viewer.open_filter(),
        (KeyCode::Char('w'), KeyModifiers::NONE) => viewer.toggle_wrap_mode(),
        (KeyCode::Char('F'), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            viewer.toggle_follow();
        }
        (KeyCode::Char('v' | 'V'), modifiers)
            if modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT =>
        {
            viewer.toggle_visual_selection();
        }
        (KeyCode::Char('J'), KeyModifiers::SHIFT) | (KeyCode::Down, KeyModifiers::SHIFT) => {
            viewer.begin_visual_selection();
            viewer.select_by(1);
        }
        (KeyCode::Char('K'), KeyModifiers::SHIFT) | (KeyCode::Up, KeyModifiers::SHIFT) => {
            viewer.begin_visual_selection();
            viewer.select_by(-1);
        }
        (KeyCode::PageDown, KeyModifiers::SHIFT) => {
            viewer.begin_visual_selection();
            viewer.scroll_page(1);
        }
        (KeyCode::Char('d' | 'D'), modifiers)
            if modifiers == KeyModifiers::CONTROL | KeyModifiers::SHIFT =>
        {
            viewer.begin_visual_selection();
            viewer.scroll_page(1);
        }
        (KeyCode::PageUp, KeyModifiers::SHIFT) => {
            viewer.begin_visual_selection();
            viewer.scroll_page(-1);
        }
        (KeyCode::Char('u' | 'U'), modifiers)
            if modifiers == KeyModifiers::CONTROL | KeyModifiers::SHIFT =>
        {
            viewer.begin_visual_selection();
            viewer.scroll_page(-1);
        }
        (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
            viewer.select_by(-1);
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
            viewer.select_by(1);
        }
        (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
            viewer.select_by(-1);
        }
        (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
            viewer.select_by(1);
        }
        (KeyCode::Char('n'), KeyModifiers::NONE) => {
            viewer.select_next_match();
        }
        (KeyCode::Char('N'), KeyModifiers::SHIFT) => {
            viewer.select_previous_match();
        }
        (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
            viewer.scroll_by(-1);
        }
        (KeyCode::Char('j'), KeyModifiers::CONTROL) => {
            viewer.scroll_by(1);
        }
        (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
            viewer.scroll_half_page(-1);
        }
        (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
            viewer.scroll_half_page(1);
        }
        (KeyCode::PageUp, _) => {
            viewer.scroll_page(-1);
        }
        (KeyCode::PageDown, _) => {
            viewer.scroll_page(1);
        }
        (KeyCode::Home, _) | (KeyCode::Char('g'), KeyModifiers::NONE) => {
            viewer.scroll_to_start();
        }
        (KeyCode::End, _) | (KeyCode::Char('G'), KeyModifiers::SHIFT) => {
            viewer.scroll_to_end();
        }
        (KeyCode::Char('z'), KeyModifiers::NONE) => {
            viewer.center_selected();
        }
        _ => return ViewerKeyResult::Unhandled,
    }
    ViewerKeyResult::Handled
}
