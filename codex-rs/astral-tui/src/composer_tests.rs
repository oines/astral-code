use std::time::Duration;
use std::time::Instant;

use codex_app_server_protocol::UserInput;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use pretty_assertions::assert_eq;

use super::ComposerMouseAction;
use super::ComposerState;
use crate::mention::MentionTarget;

#[test]
fn edits_at_a_utf8_cursor_and_normalizes_paste_newlines() {
    let mut composer = ComposerState::default();
    composer.insert_text("a中c");
    assert!(composer.move_left());
    composer.insert_char('文');
    assert_eq!(composer.text(), "a中文c");
    assert_eq!(composer.cursor(), "a中文".len());

    assert!(composer.backspace());
    assert_eq!(composer.text(), "a中c");
    assert_eq!(composer.cursor(), "a中".len());

    composer.insert_text("\r\nnext\rlast");
    assert_eq!(composer.text(), "a中\nnext\nlastc");
}

#[test]
fn horizontal_home_end_delete_and_word_delete_preserve_boundaries() {
    let mut composer = ComposerState::default();
    composer.replace("one two\n三 four");
    assert!(composer.move_home());
    assert_eq!(composer.cursor(), "one two\n".len());
    assert!(composer.delete());
    assert_eq!(composer.text(), "one two\n four");
    assert!(composer.move_end());
    assert_eq!(composer.cursor(), composer.text().len());
    assert!(composer.delete_word_left());
    assert_eq!(composer.text(), "one two\n ");
}

#[test]
fn vertical_navigation_preserves_the_preferred_column() {
    let mut composer = ComposerState::default();
    composer.replace("abcd\n中\n12345");
    assert!(composer.move_home());
    assert!(composer.move_right());
    assert!(composer.move_right());
    assert!(composer.move_right());
    assert_eq!(composer.cursor(), "abcd\n中\n123".len());

    assert!(composer.move_up());
    assert_eq!(composer.cursor(), "abcd\n中".len());
    assert!(composer.move_up());
    assert_eq!(composer.cursor(), 3);
    assert!(composer.move_down());
    assert_eq!(composer.cursor(), "abcd\n中".len());
}

#[test]
fn readline_navigation_kill_and_yank_match_the_reference_tuis() {
    let mut composer = ComposerState::default();
    composer.replace("first\nsecond");

    assert!(composer.edit_key(modified_char('a', KeyModifiers::CONTROL)));
    assert_eq!(composer.cursor(), "first\n".len());
    assert!(composer.edit_key(modified_char('a', KeyModifiers::CONTROL)));
    assert_eq!(composer.cursor(), 0);
    assert!(composer.edit_key(modified_char('e', KeyModifiers::CONTROL)));
    assert_eq!(composer.cursor(), "first".len());
    assert!(composer.edit_key(modified_char('e', KeyModifiers::CONTROL)));
    assert_eq!(composer.cursor(), "first\nsecond".len());

    assert!(composer.edit_key(modified_char('u', KeyModifiers::CONTROL)));
    assert_eq!(composer.text(), "first\n");
    assert!(composer.edit_key(modified_char('y', KeyModifiers::CONTROL)));
    assert_eq!(composer.text(), "first\nsecond");

    composer.replace("one.two");
    assert!(composer.edit_key(modified_char('b', KeyModifiers::ALT)));
    assert_eq!(composer.cursor(), "one.".len());
    assert!(composer.edit_key(modified_char('b', KeyModifiers::ALT)));
    assert_eq!(composer.cursor(), "one".len());
    composer.move_end();
    assert!(composer.edit_key(modified_char('w', KeyModifiers::CONTROL)));
    assert_eq!(composer.text(), "");
    assert!(composer.edit_key(modified_char('y', KeyModifiers::CONTROL)));
    assert_eq!(composer.text(), "one.two");
}

#[test]
fn undo_redo_groups_typing_and_restores_structured_mentions() {
    let mut composer = ComposerState::default();
    composer.insert_text("hello");
    composer.insert_char(' ');
    composer.insert_text("world");

    assert!(composer.edit_key(modified_char('z', KeyModifiers::CONTROL)));
    assert_eq!(composer.text(), "hello ");
    assert!(composer.edit_key(modified_char('z', KeyModifiers::CONTROL)));
    assert_eq!(composer.text(), "hello");
    assert!(composer.edit_key(modified_char('r', KeyModifiers::CONTROL)));
    assert_eq!(composer.text(), "hello ");
    assert!(composer.edit_key(modified_char(
        'Z',
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    )));
    assert_eq!(composer.text(), "hello world");

    composer.replace("$rev");
    let (insert_text, target) = skill_mention();
    composer.insert_mention(0..4, insert_text, target);
    assert!(composer.edit_key(modified_char('z', KeyModifiers::CONTROL)));
    assert_eq!(composer.text(), "$rev");
    assert!(composer.edit_key(modified_char('r', KeyModifiers::CONTROL)));
    assert_eq!(
        composer.take_submission().user_input(),
        vec![
            UserInput::Text {
                text: "$review ".to_string(),
                text_elements: Vec::new(),
            },
            UserInput::Skill {
                name: "review".to_string(),
                path: "/skills/review/SKILL.md".into(),
            },
        ]
    );
}

#[test]
fn selected_mentions_survive_edits_before_them_and_project_structured_input() {
    let mut composer = ComposerState::default();
    composer.replace("use $rev");
    let (insert_text, target) = skill_mention();
    composer.insert_mention(4..8, insert_text, target);
    assert_eq!(composer.text(), "use $review ");
    composer.move_home();
    composer.insert_text("please ");

    let submission = composer.take_submission();
    assert_eq!(submission.text(), "please use $review ");
    assert_eq!(
        submission.user_input(),
        vec![
            UserInput::Text {
                text: "please use $review ".to_string(),
                text_elements: Vec::new(),
            },
            UserInput::Skill {
                name: "review".to_string(),
                path: "/skills/review/SKILL.md".into(),
            },
        ]
    );
}

#[test]
fn editing_inside_a_selected_mention_drops_its_structured_binding() {
    let mut composer = ComposerState::default();
    composer.replace("$rev");
    let (insert_text, target) = skill_mention();
    composer.insert_mention(0..4, insert_text, target);
    assert!(composer.move_left());
    assert!(composer.move_left());
    composer.insert_char('x');

    let submission = composer.take_submission();
    assert_eq!(
        submission.user_input(),
        vec![UserInput::Text {
            text: "$reviexw ".to_string(),
            text_elements: Vec::new(),
        }]
    );
}

#[test]
fn mouse_selection_copies_and_replaces_the_selected_buffer_range() {
    let mut composer = ComposerState::default();
    composer.replace("hello world");
    let now = Instant::now();

    assert_eq!(
        composer.handle_mouse(
            mouse(MouseEventKind::Down(MouseButton::Left), 4, 2),
            Some(6),
            now,
        ),
        ComposerMouseAction::Redraw
    );
    assert_eq!(
        composer.handle_mouse(
            mouse(MouseEventKind::Drag(MouseButton::Left), 9, 2),
            Some(11),
            now + Duration::from_millis(20),
        ),
        ComposerMouseAction::Redraw
    );
    assert_eq!(
        composer.handle_mouse(
            mouse(MouseEventKind::Up(MouseButton::Left), 9, 2),
            None,
            now + Duration::from_millis(40),
        ),
        ComposerMouseAction::Copy("world".to_string())
    );
    assert_eq!(composer.selection_range(), Some(6..11));

    composer.insert_text("Astral");
    assert_eq!(composer.text(), "hello Astral");
    assert_eq!(composer.selection_range(), None);
    assert!(composer.edit_key(modified_char('z', KeyModifiers::CONTROL)));
    assert_eq!(composer.text(), "hello world");

    assert_eq!(
        composer.handle_mouse(
            mouse(MouseEventKind::Down(MouseButton::Left), 3, 3),
            Some(1),
            now + Duration::from_millis(100),
        ),
        ComposerMouseAction::Redraw
    );
    let _ = composer.handle_mouse(
        mouse(MouseEventKind::Up(MouseButton::Left), 3, 3),
        None,
        now + Duration::from_millis(120),
    );
    assert_eq!(
        composer.handle_mouse(
            mouse(MouseEventKind::Down(MouseButton::Left), 3, 3),
            Some(1),
            now + Duration::from_millis(200),
        ),
        ComposerMouseAction::Copy("hello".to_string())
    );
    let _ = composer.handle_mouse(
        mouse(MouseEventKind::Up(MouseButton::Left), 3, 3),
        None,
        now + Duration::from_millis(220),
    );
    assert_eq!(
        composer.handle_mouse(
            mouse(MouseEventKind::Down(MouseButton::Left), 3, 3),
            Some(1),
            now + Duration::from_millis(300),
        ),
        ComposerMouseAction::Copy("hello world".to_string())
    );
    assert!(composer.edit_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE,)));
    assert_eq!(composer.text(), "");
}

fn skill_mention() -> (String, MentionTarget) {
    (
        "$review".to_string(),
        MentionTarget::Skill {
            name: "review".to_string(),
            path: "/skills/review/SKILL.md".into(),
        },
    )
}

fn modified_char(character: char, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(character), modifiers)
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}
