use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use pretty_assertions::assert_eq;
use ratatui::layout::Rect;

use super::BlockViewerFrame;
use super::BlockViewerMouseAction;
use super::ViewerRowGeometry;
use super::ViewerState;

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

fn observe_frame(
    state: &mut ViewerState,
    popup: Rect,
    content: Rect,
    close: Rect,
    logical_lines: Vec<String>,
    is_running: bool,
) {
    let edit_copy_lines = vec![None; logical_lines.len()];
    let row_geometry = logical_lines
        .iter()
        .enumerate()
        .map(|(item, line)| {
            ViewerRowGeometry::new(item, 0, u16::try_from(line.len()).unwrap_or(u16::MAX))
        })
        .collect();
    let rendered_rows = logical_lines.clone();
    state.observe_frame(BlockViewerFrame {
        popup_area: popup,
        content_area: content,
        close_button: close,
        logical_lines,
        edit_copy_lines,
        row_geometry,
        rendered_rows,
        is_running,
    });
}

#[test]
fn viewer_scroll_is_clamped_to_the_observed_content() {
    let mut state = ViewerState::new(false);
    observe_frame(
        &mut state,
        Rect::new(1, 1, 20, 10),
        Rect::new(3, 3, 16, 4),
        Rect::new(17, 1, 3, 1),
        lines(12),
        false,
    );

    assert_eq!(state.selected_item(), Some(0));
    assert!(state.scroll_by(50));
    assert_eq!(state.scroll_offset(), 8);
    assert_eq!(state.selected_item(), Some(8));
    assert!(state.scroll_page(-1));
    assert_eq!(state.scroll_offset(), 4);
    assert_eq!(state.selected_item(), Some(4));
    assert!(state.scroll_to_start());
    assert_eq!(state.scroll_offset(), 0);
    assert_eq!(state.selected_item(), Some(0));
    assert!(state.scroll_to_end());
    assert_eq!(state.scroll_offset(), 8);
    assert_eq!(state.selected_item(), Some(11));
}

#[test]
fn viewer_pointer_uses_the_rendered_modal_geometry() {
    let mut state = ViewerState::new(false);
    observe_frame(
        &mut state,
        Rect::new(2, 2, 30, 12),
        Rect::new(5, 4, 24, 7),
        Rect::new(27, 2, 3, 1),
        lines(20),
        false,
    );
    state.observe_scrollbar_area(Some(Rect::new(30, 4, 1, 7)));

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
        state.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 30, 10)),
        BlockViewerMouseAction::Redraw
    );
    assert_eq!(state.scroll_offset(), 13);
    assert_eq!(state.selected_item(), Some(19));
    assert_eq!(
        state.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 30, 4)),
        BlockViewerMouseAction::Redraw
    );
    assert_eq!(state.scroll_offset(), 0);
    assert_eq!(state.selected_item(), Some(0));
    assert_eq!(
        state.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 30, 4)),
        BlockViewerMouseAction::Redraw
    );
    assert_eq!(
        state.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 5, 6)),
        BlockViewerMouseAction::Redraw
    );
    assert_eq!(
        state.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 9, 6)),
        BlockViewerMouseAction::Redraw
    );
    assert_eq!(
        state.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 9, 6)),
        BlockViewerMouseAction::Copy("line ".to_string())
    );
    assert_eq!(state.selected_item(), Some(0));
}

#[test]
fn viewer_search_uses_rendered_line_order_and_wraps_matches() {
    let mut state = ViewerState::new(false);
    observe_frame(
        &mut state,
        Rect::new(1, 1, 30, 12),
        Rect::new(3, 3, 24, 5),
        Rect::new(27, 1, 3, 1),
        vec![
            "alpha".to_string(),
            "first beta".to_string(),
            "middle".to_string(),
            "second beta".to_string(),
        ],
        false,
    );

    state.open_search();
    for character in "beta".chars() {
        state.handle_query_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(character),
            KeyModifiers::NONE,
        ));
    }
    assert_eq!(state.selected_item(), Some(1));
    assert!(state.select_next_match());
    assert_eq!(state.selected_item(), Some(3));
    assert!(state.select_next_match());
    assert_eq!(state.selected_item(), Some(1));
}

#[test]
fn viewer_filter_keeps_only_matching_rendered_lines() {
    let mut state = ViewerState::new(false);
    state.observe_frame(BlockViewerFrame {
        popup_area: Rect::new(1, 1, 30, 12),
        content_area: Rect::new(3, 3, 24, 5),
        close_button: Rect::new(27, 1, 3, 1),
        logical_lines: vec!["alpha continued".to_string(), "beta".to_string()],
        edit_copy_lines: vec![None, None],
        row_geometry: vec![
            ViewerRowGeometry::new(0, 0, 5),
            ViewerRowGeometry::new(0, 6, 15),
            ViewerRowGeometry::new(1, 0, 4),
        ],
        rendered_rows: vec![
            "alpha".to_string(),
            "continued".to_string(),
            "beta".to_string(),
        ],
        is_running: false,
    });

    state.open_filter();
    for character in "alpha".chars() {
        state.handle_query_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(character),
            KeyModifiers::NONE,
        ));
    }

    assert_eq!(
        (0..3)
            .map(|line| state.rendered_row(line))
            .collect::<Vec<_>>(),
        vec![Some("alpha"), Some("continued"), None]
    );
}

#[test]
fn viewer_visual_selection_copies_the_rendered_line_range() {
    let mut state = ViewerState::new(false);
    observe_frame(
        &mut state,
        Rect::new(1, 1, 30, 12),
        Rect::new(3, 3, 24, 5),
        Rect::new(27, 1, 3, 1),
        lines(5),
        false,
    );

    state.select_by(1);
    state.begin_visual_selection();
    state.select_by(2);

    assert_eq!(
        state.take_visual_selection_text(&astral_tui_scrollback::PresentationBlock::Assistant {
            text: String::new(),
        }),
        Some("line 1\nline 2\nline 3".to_string())
    );
    assert!(!state.visual_selection_active());
}

#[test]
fn running_viewer_follows_live_content_until_navigation_pauses_it() {
    let mut state = ViewerState::new(true);
    let popup = Rect::new(1, 1, 20, 10);
    let content = Rect::new(3, 3, 16, 4);
    let close = Rect::new(17, 1, 3, 1);

    observe_frame(&mut state, popup, content, close, lines(12), true);
    assert_eq!((state.scroll_offset(), state.selected_item()), (8, None));

    observe_frame(&mut state, popup, content, close, lines(14), true);
    assert_eq!((state.scroll_offset(), state.selected_item()), (10, None));

    assert!(state.select_by(-1));
    assert_eq!(state.selected_item(), Some(12));
    observe_frame(&mut state, popup, content, close, lines(16), true);
    assert_eq!(state.selected_item(), Some(12));
    assert!(state.scroll_offset() < 12);

    assert!(state.toggle_follow());
    observe_frame(&mut state, popup, content, close, lines(18), true);
    assert_eq!((state.scroll_offset(), state.selected_item()), (14, None));

    observe_frame(&mut state, popup, content, close, lines(18), false);
    assert_eq!(
        (state.scroll_offset(), state.selected_item()),
        (14, Some(17))
    );
}
