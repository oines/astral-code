use std::time::Duration;
use std::time::Instant;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyEventState;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use pretty_assertions::assert_eq;
use ratatui::layout::Rect;

use super::ChoiceList;
use super::ChoiceListOutcome;

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
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
fn keyboard_navigation_stays_bounded_and_activates_exact_choices() {
    let mut choices = ChoiceList::default();

    assert_eq!(
        choices.handle_key(key(KeyCode::Up, KeyModifiers::NONE), 3),
        ChoiceListOutcome::Changed
    );
    assert_eq!(choices.selected(), 0);
    assert_eq!(
        choices.handle_key(key(KeyCode::Tab, KeyModifiers::NONE), 3),
        ChoiceListOutcome::Changed
    );
    assert_eq!(choices.selected(), 1);
    assert_eq!(
        choices.handle_key(key(KeyCode::BackTab, KeyModifiers::SHIFT), 3),
        ChoiceListOutcome::Changed
    );
    assert_eq!(choices.selected(), 0);
    assert_eq!(
        choices.handle_key(key(KeyCode::Char('3'), KeyModifiers::NONE), 3),
        ChoiceListOutcome::Activate(2)
    );
    assert_eq!(choices.selected(), 2);
    assert_eq!(
        choices.handle_key(key(KeyCode::Down, KeyModifiers::NONE), 3),
        ChoiceListOutcome::Changed
    );
    assert_eq!(choices.selected(), 2);
    assert_eq!(
        choices.handle_key(key(KeyCode::Enter, KeyModifiers::NONE), 3),
        ChoiceListOutcome::Activate(2)
    );
    assert_eq!(
        choices.handle_key(key(KeyCode::Char('9'), KeyModifiers::NONE), 3),
        ChoiceListOutcome::Unchanged
    );
    assert_eq!(
        choices.handle_key(
            KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Release,
                state: KeyEventState::NONE,
            },
            3,
        ),
        ChoiceListOutcome::Unchanged
    );
}

#[test]
fn mouse_navigation_tracks_hover_scroll_and_double_click() {
    let mut choices = ChoiceList::default();
    choices.record_hit(Rect::new(2, 4, 10, 1));
    choices.record_hit(Rect::new(2, 5, 10, 1));
    let now = Instant::now();

    assert_eq!(
        choices.handle_mouse(mouse(MouseEventKind::Moved, 4, 5), now, 2),
        ChoiceListOutcome::Changed
    );
    assert_eq!(choices.hovered, Some(1));
    assert_eq!(
        choices.handle_mouse(mouse(MouseEventKind::ScrollDown, 0, 0), now, 2),
        ChoiceListOutcome::Changed
    );
    assert_eq!(choices.selected(), 1);
    assert_eq!(
        choices.handle_mouse(mouse(MouseEventKind::ScrollUp, 0, 0), now, 2),
        ChoiceListOutcome::Changed
    );
    assert_eq!(choices.selected(), 0);

    let click = mouse(MouseEventKind::Down(MouseButton::Left), 4, 5);
    assert_eq!(
        choices.handle_mouse(click, now, 2),
        ChoiceListOutcome::Changed
    );
    assert_eq!(
        choices.handle_mouse(click, now + Duration::from_millis(100), 2),
        ChoiceListOutcome::Activate(1)
    );
    assert_eq!(choices.selected(), 1);
    assert_eq!(
        choices.handle_mouse(mouse(MouseEventKind::ScrollDown, 0, 0), now, 0),
        ChoiceListOutcome::Unchanged
    );
}
