use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::InputAction;
use super::handle_key;
use crate::RequestResolution;
use crate::SurfaceActivity;
use crate::SurfaceState;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn request(value: serde_json::Value) -> ServerRequest {
    serde_json::from_value(value).expect("valid server request")
}

#[test]
fn composer_submit_and_interrupt_are_distinct_actions() {
    let mut state = SurfaceState::new("thread-1");
    for character in "hello".chars() {
        assert_eq!(
            handle_key(&mut state, key(KeyCode::Char(character))),
            InputAction::Redraw
        );
    }
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Submit("hello".to_string())
    );

    state.set_activity(SurfaceActivity::Working);
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ),
        InputAction::Interrupt
    );
}

#[test]
fn command_session_approval_preserves_typed_decision() {
    let mut state = SurfaceState::new("thread-1");
    state.pending_requests_mut().note(request(json!({
        "method": "item/commandExecution/requestApproval",
        "id": 7,
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "call-1",
            "startedAtMs": 100,
            "availableDecisions": ["accept", "acceptForSession", "decline", "cancel"]
        }
    })));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('a'))),
        InputAction::Resolve(RequestResolution::Success {
            request_id: RequestId::Integer(7),
            result: json!({"decision": "acceptForSession"}),
        })
    );
}

#[test]
fn user_input_supports_multiple_question_answers() {
    let mut state = SurfaceState::new("thread-1");
    state.pending_requests_mut().note(request(json!({
        "method": "item/tool/requestUserInput",
        "id": "question-1",
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "call-1",
            "questions": [
                {
                    "id": "language",
                    "header": "Language",
                    "question": "Which language?",
                    "options": null
                },
                {
                    "id": "style",
                    "header": "Style",
                    "question": "Which style?",
                    "options": null
                }
            ]
        }
    })));
    state.composer_mut().push_str("Rust | concise");

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Resolve(RequestResolution::Success {
            request_id: RequestId::String("question-1".to_string()),
            result: json!({
                "answers": {
                    "language": {"answers": ["Rust"]},
                    "style": {"answers": ["concise"]}
                }
            }),
        })
    );
}
