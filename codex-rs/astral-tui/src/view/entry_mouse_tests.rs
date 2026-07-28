use std::ops::Range;
use std::time::Duration;
use std::time::Instant;

use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use pretty_assertions::assert_eq;
use ratatui::layout::Rect;

use super::EntryMouseAction;
use super::EntryMouseState;
use super::MULTI_CLICK_TIMEOUT;
use crate::view::ScrollbackViewport;
use crate::view::transcript::TranscriptLayout;
use crate::view::transcript::TranscriptSection;

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn state(section_lines: Range<usize>) -> EntryMouseState {
    let layout = TranscriptLayout {
        lines: Vec::new(),
        sections: vec![TranscriptSection {
            item_id: "turn-1\0tool-1".to_string(),
            lines: section_lines,
        }],
        selectable_ranges: Vec::new(),
    };
    let mut state = EntryMouseState::default();
    state.observe(
        &layout,
        ScrollbackViewport::from_first(
            /*total_lines*/ 20, /*viewport_lines*/ 8, /*first_visible_line*/ 5,
        ),
        Rect::new(2, 3, 40, 8),
    );
    state
}

#[test]
fn click_uses_the_visible_viewport_offset() {
    let mut state = state(7..10);
    let now = Instant::now();

    assert_eq!(
        state.handle_mouse_at(mouse(MouseEventKind::Down(MouseButton::Left), 4, 5), now),
        EntryMouseAction::Ignored
    );
    assert_eq!(
        state.handle_mouse_at(
            mouse(MouseEventKind::Up(MouseButton::Left), 4, 5),
            now + Duration::from_millis(1)
        ),
        EntryMouseAction::Select("turn-1\0tool-1".to_string())
    );
}

#[test]
fn second_click_toggles_but_a_drag_does_not() {
    let mut state = state(7..10);
    let now = Instant::now();
    let down = mouse(MouseEventKind::Down(MouseButton::Left), 4, 5);
    let up = mouse(MouseEventKind::Up(MouseButton::Left), 4, 5);

    state.handle_mouse_at(down, now);
    state.handle_mouse_at(up, now + Duration::from_millis(1));
    state.handle_mouse_at(down, now + Duration::from_millis(2));
    assert_eq!(
        state.handle_mouse_at(up, now + MULTI_CLICK_TIMEOUT - Duration::from_millis(1)),
        EntryMouseAction::Toggle("turn-1\0tool-1".to_string())
    );

    state.handle_mouse_at(down, now + Duration::from_secs(1));
    state.handle_mouse_at(
        mouse(MouseEventKind::Drag(MouseButton::Left), 8, 5),
        now + Duration::from_secs(1) + Duration::from_millis(1),
    );
    assert_eq!(
        state.handle_mouse_at(up, now + Duration::from_secs(1) + Duration::from_millis(2)),
        EntryMouseAction::Ignored
    );
}
