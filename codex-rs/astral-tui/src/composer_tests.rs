use codex_app_server_protocol::UserInput;
use pretty_assertions::assert_eq;

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

fn skill_mention() -> (String, MentionTarget) {
    (
        "$review".to_string(),
        MentionTarget::Skill {
            name: "review".to_string(),
            path: "/skills/review/SKILL.md".into(),
        },
    )
}
