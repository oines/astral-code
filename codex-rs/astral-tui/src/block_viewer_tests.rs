use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use pretty_assertions::assert_eq;
use ratatui::layout::Rect;

use super::BlockViewerMouseAction;
use super::BlockViewerState;

fn lines(count: usize) -> Vec<String> {
    (0..count).map(|line| format!("line {line}")).collect()
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

#[test]
fn viewer_scroll_is_clamped_to_the_observed_content() {
    let mut state = BlockViewerState::new("turn\0entry-1".to_string());
    state.observe_frame(
        Rect::new(1, 1, 20, 10),
        Rect::new(3, 3, 16, 4),
        Rect::new(17, 1, 3, 1),
        lines(12),
    );

    assert_eq!(state.selected_line(), Some(0));
    assert!(state.scroll_by(50));
    assert_eq!(state.scroll_offset(), 8);
    assert_eq!(state.selected_line(), Some(8));
    assert!(state.scroll_page(-1));
    assert_eq!(state.scroll_offset(), 4);
    assert_eq!(state.selected_line(), Some(4));
    assert!(state.scroll_to_start());
    assert_eq!(state.scroll_offset(), 0);
    assert_eq!(state.selected_line(), Some(0));
    assert!(state.scroll_to_end());
    assert_eq!(state.scroll_offset(), 8);
    assert_eq!(state.selected_line(), Some(11));
}

#[test]
fn viewer_pointer_uses_the_rendered_modal_geometry() {
    let mut state = BlockViewerState::new("turn\0entry-1".to_string());
    state.observe_frame(
        Rect::new(2, 2, 30, 12),
        Rect::new(5, 4, 24, 7),
        Rect::new(27, 2, 3, 1),
        lines(20),
    );

    assert_eq!(
        state.handle_mouse(mouse(MouseEventKind::Moved, 28, 2)),
        BlockViewerMouseAction::Redraw
    );
    assert!(state.close_hovered());
    assert_eq!(
        state.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 28, 2)),
        BlockViewerMouseAction::Close
    );
    assert_eq!(
        state.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 0, 0)),
        BlockViewerMouseAction::Close
    );
    assert_eq!(
        state.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 8, 6)),
        BlockViewerMouseAction::Redraw
    );
    assert_eq!(state.selected_line(), Some(2));
}

#[test]
fn viewer_search_uses_rendered_line_order_and_wraps_matches() {
    let mut state = BlockViewerState::new("turn\0entry-1".to_string());
    state.observe_frame(
        Rect::new(1, 1, 30, 12),
        Rect::new(3, 3, 24, 5),
        Rect::new(27, 1, 3, 1),
        vec![
            "alpha".to_string(),
            "first beta".to_string(),
            "middle".to_string(),
            "second beta".to_string(),
        ],
    );

    state.open_search();
    for character in "beta".chars() {
        state.handle_search_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(character),
            KeyModifiers::NONE,
        ));
    }
    assert_eq!(state.selected_line(), Some(1));
    assert!(state.select_next_match());
    assert_eq!(state.selected_line(), Some(3));
    assert!(state.select_next_match());
    assert_eq!(state.selected_line(), Some(1));
}
