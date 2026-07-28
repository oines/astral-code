use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;

use codex_app_server_protocol::ToolRequestUserInputAnswer;
use codex_app_server_protocol::ToolRequestUserInputOption;
use codex_app_server_protocol::ToolRequestUserInputParams;
use codex_app_server_protocol::ToolRequestUserInputQuestion;
use codex_app_server_protocol::ToolRequestUserInputResponse;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use pretty_assertions::assert_eq;
use ratatui::layout::Rect;

use super::RequestUserInputEvent;
use super::RequestUserInputHit;
use super::RequestUserInputState;

fn params() -> ToolRequestUserInputParams {
    ToolRequestUserInputParams {
        thread_id: "thread-1".to_string(),
        turn_id: "turn-1".to_string(),
        item_id: "question-1".to_string(),
        questions: vec![ToolRequestUserInputQuestion {
            id: "scope".to_string(),
            header: "Scope".to_string(),
            question: "Which scope?".to_string(),
            is_other: false,
            is_secret: false,
            options: Some(vec![
                ToolRequestUserInputOption {
                    label: "Workspace".to_string(),
                    description: "Only this repository".to_string(),
                },
                ToolRequestUserInputOption {
                    label: "Shared".to_string(),
                    description: "Common runtime".to_string(),
                },
            ]),
        }],
    }
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
fn option_requires_a_second_click_to_submit() {
    let params = params();
    let mut state = RequestUserInputState::default();
    state.sync(&params);
    state.observe_rows(vec![(
        RequestUserInputHit::Option(1),
        Rect::new(3, 5, 24, 1),
    )]);
    let now = Instant::now();

    assert_eq!(
        state.handle_mouse_at(
            &params,
            mouse(MouseEventKind::Down(MouseButton::Left), 4, 5),
            now,
        ),
        RequestUserInputEvent::Redraw
    );
    assert_eq!(state.selected_option(), Some(1));
    assert_eq!(state.option_committed(), true);
    assert_eq!(
        state.handle_mouse_at(
            &params,
            mouse(MouseEventKind::Down(MouseButton::Left), 4, 5),
            now + Duration::from_millis(1),
        ),
        RequestUserInputEvent::Submit(ToolRequestUserInputResponse {
            answers: HashMap::from([(
                "scope".to_string(),
                ToolRequestUserInputAnswer {
                    answers: vec!["Shared".to_string()],
                },
            )]),
        })
    );
}

#[test]
fn pointer_hover_tracks_only_rendered_rows() {
    let params = params();
    let mut state = RequestUserInputState::default();
    state.sync(&params);
    state.observe_rows(vec![(
        RequestUserInputHit::Option(0),
        Rect::new(3, 5, 24, 1),
    )]);

    assert_eq!(
        state.handle_mouse_at(&params, mouse(MouseEventKind::Moved, 4, 5), Instant::now(),),
        RequestUserInputEvent::Redraw
    );
    assert_eq!(state.hovered(), Some(RequestUserInputHit::Option(0)));
    assert_eq!(
        state.handle_mouse_at(&params, mouse(MouseEventKind::Moved, 4, 8), Instant::now(),),
        RequestUserInputEvent::Redraw
    );
    assert_eq!(state.hovered(), None);
}

#[test]
fn escape_closes_option_notes_before_cancelling_the_request() {
    let params = params();
    let mut state = RequestUserInputState::default();
    state.sync(&params);

    assert_eq!(
        state.handle_key(&params, KeyEvent::from(KeyCode::Tab)),
        RequestUserInputEvent::Redraw
    );
    assert_eq!(state.handle_paste(&params, "detail"), true);
    assert_eq!(
        state.handle_key(&params, KeyEvent::from(KeyCode::Esc)),
        RequestUserInputEvent::Redraw
    );
    assert_eq!(state.notes_visible(), false);
    assert_eq!(state.editor(), "");
    assert_eq!(
        state.handle_key(&params, KeyEvent::from(KeyCode::Esc)),
        RequestUserInputEvent::Cancel
    );
}

#[test]
fn shift_x_explicitly_cancels_from_any_question_state() {
    let params = params();
    let mut state = RequestUserInputState::default();
    state.sync(&params);

    assert_eq!(
        state.handle_key(
            &params,
            KeyEvent::new(KeyCode::Char('X'), KeyModifiers::SHIFT),
        ),
        RequestUserInputEvent::Cancel
    );
}
