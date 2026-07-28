use std::time::Duration;
use std::time::Instant;

use codex_app_server_protocol::CommandExecutionApprovalDecision;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use pretty_assertions::assert_eq;
use ratatui::layout::Rect;
use serde_json::json;

use super::RequestChoiceEvent;
use super::RequestChoiceId;
use super::RequestChoiceState;
use super::cancel_response;
use super::response_for;
use crate::PendingRequest;
use crate::PendingRequestResponse;

fn request(value: serde_json::Value) -> PendingRequest {
    let request: ServerRequest = serde_json::from_value(value).expect("valid request");
    PendingRequest::from(request)
}

fn command_request(available_decisions: serde_json::Value) -> PendingRequest {
    request(json!({
        "method": "item/commandExecution/requestApproval",
        "id": 8,
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "call-1",
            "startedAtMs": 100,
            "availableDecisions": available_decisions
        }
    }))
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
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
fn command_choices_follow_available_typed_decisions() {
    let request = command_request(json!(["accept", "decline"]));
    let mut state = RequestChoiceState::default();
    state.sync(Some(&request));

    assert_eq!(
        state
            .choices()
            .iter()
            .map(|choice| (choice.id, choice.label))
            .collect::<Vec<_>>(),
        vec![
            (RequestChoiceId::CommandAccept, "Allow once"),
            (RequestChoiceId::CommandDecline, "Deny"),
        ]
    );
    assert_eq!(
        state.handle_key(key(KeyCode::Char('a'))),
        RequestChoiceEvent::None
    );
    assert_eq!(
        state.handle_key(key(KeyCode::Char('2'))),
        RequestChoiceEvent::Activate(RequestChoiceId::CommandDecline)
    );
}

#[test]
fn keyboard_navigation_activates_the_selected_typed_response() {
    let request = command_request(json!(["accept", "acceptForSession", "decline", "cancel"]));
    let mut state = RequestChoiceState::default();
    state.sync(Some(&request));

    assert_eq!(
        state.handle_key(key(KeyCode::Down)),
        RequestChoiceEvent::Redraw
    );
    let RequestChoiceEvent::Activate(choice) = state.handle_key(key(KeyCode::Enter)) else {
        panic!("enter should activate the selected approval");
    };
    assert_eq!(
        response_for(&request, choice),
        Some(PendingRequestResponse::CommandExecution(
            CommandExecutionApprovalDecision::AcceptForSession,
        ))
    );
}

#[test]
fn escape_preserves_request_specific_cancel_semantics() {
    let cancellable = command_request(json!(["accept", "cancel"]));
    assert_eq!(
        cancel_response(&cancellable),
        Some(PendingRequestResponse::CommandExecution(
            CommandExecutionApprovalDecision::Cancel,
        ))
    );

    let not_cancellable = command_request(json!(["accept", "decline"]));
    assert_eq!(cancel_response(&not_cancellable), None);
    assert_eq!(not_cancellable.request_id(), &RequestId::Integer(8));
}

#[test]
fn pointer_requires_a_second_click_to_activate() {
    let request = command_request(json!(["accept", "decline"]));
    let mut state = RequestChoiceState::default();
    state.sync(Some(&request));
    state.observe_rows(vec![
        (0, Rect::new(2, 4, 20, 1)),
        (1, Rect::new(2, 5, 20, 1)),
    ]);
    let now = Instant::now();

    assert_eq!(
        state.handle_mouse_at(mouse(MouseEventKind::Down(MouseButton::Left), 4, 5), now),
        RequestChoiceEvent::Redraw
    );
    assert_eq!(state.selected(), Some(1));
    assert_eq!(
        state.handle_mouse_at(
            mouse(MouseEventKind::Down(MouseButton::Left), 4, 5),
            now + Duration::from_millis(100),
        ),
        RequestChoiceEvent::Activate(RequestChoiceId::CommandDecline)
    );
}

#[test]
fn hover_tracks_only_visible_choice_rows() {
    let request = command_request(json!(["accept", "decline"]));
    let mut state = RequestChoiceState::default();
    state.sync(Some(&request));
    state.observe_rows(vec![(0, Rect::new(2, 4, 20, 1))]);

    assert_eq!(
        state.handle_mouse(mouse(MouseEventKind::Moved, 4, 4)),
        RequestChoiceEvent::Redraw
    );
    assert_eq!(state.hovered(), Some(0));
    assert_eq!(
        state.handle_mouse(mouse(MouseEventKind::Moved, 4, 8)),
        RequestChoiceEvent::Redraw
    );
    assert_eq!(state.hovered(), None);
}
