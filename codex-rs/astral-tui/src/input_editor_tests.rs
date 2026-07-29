use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;

use super::InputAction;
use super::handle_key;
use super::handle_paste;
use crate::SurfaceState;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn event_mapping_edits_inside_utf8_text() {
    let mut state = SurfaceState::new("thread-1");
    state.set_composer("a中c");

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Left)),
        InputAction::Redraw
    );
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('文'))),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "a中文c");
    assert_eq!(state.composer_cursor(), "a中文".len());

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Home)),
        InputAction::Redraw
    );
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Delete)),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "中文c");

    state.set_composer("ab");
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Home)),
        InputAction::Redraw
    );
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)
        ),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "b");
}

#[test]
fn paste_and_modified_enter_insert_at_the_cursor() {
    let mut state = SurfaceState::new("thread-1");
    state.set_composer("tail");
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Home)),
        InputAction::Redraw
    );
    assert_eq!(
        handle_paste(&mut state, "one\r\ntwo\r"),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "one\ntwo\ntail");

    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)
        ),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "one\ntwo\n\ntail");
}

#[test]
fn moving_away_from_the_end_closes_slash_completion() {
    let mut state = SurfaceState::new("thread-1");
    state.set_composer("/mo");
    assert!(state.slash().open);

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Left)),
        InputAction::Redraw
    );
    assert!(!state.slash().open);
}
