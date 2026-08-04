use std::time::Duration;
use std::time::Instant;

use codex_app_server_protocol::CommandExecutionRequestApprovalParams;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::PromptInteractionHost;
use super::PromptInteractionOutcome;
use crate::PendingInteractions;

#[test]
fn front_approval_owns_input_until_its_exact_request_is_resolved() {
    let mut pending = PendingInteractions::new("thread-1");
    pending.observe_request(command_request(1, "cargo test -p astral-tui"));
    pending.observe_request(command_request(2, "cargo fmt"));
    let mut host = PromptInteractionHost::new();
    assert!(host.sync(&pending));
    assert_eq!(host.queue_len(), 2);

    let area = Rect::new(0, 0, 78, host.desired_height(78, 14));
    let mut buffer = Buffer::empty(area);
    host.render(&mut buffer, area);
    insta::assert_snapshot!(buffer_text(&buffer));

    assert_eq!(
        host.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        PromptInteractionOutcome::Changed
    );
    let PromptInteractionOutcome::Submit(cancel) =
        host.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
    else {
        panic!("selected decision should submit");
    };
    assert_eq!(cancel.request_id, RequestId::Integer(1));
    assert_eq!(cancel.result, serde_json::json!({ "decision": "cancel" }));

    pending
        .begin_response(&RequestId::Integer(1))
        .expect("first response should start");
    assert!(host.sync(&pending));
    assert_eq!(
        host.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        PromptInteractionOutcome::Unchanged
    );
    pending.response_succeeded(&RequestId::Integer(1));
    assert!(host.sync(&pending));
    assert_eq!(host.queue_len(), 1);
    host.render(&mut buffer, area);

    let first = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 2,
        row: area.height - 3,
        modifiers: KeyModifiers::NONE,
    };
    let now = Instant::now();
    assert_eq!(
        host.handle_mouse_event_at(first, now),
        PromptInteractionOutcome::Changed
    );
    let PromptInteractionOutcome::Submit(accept) =
        host.handle_mouse_event_at(first, now + Duration::from_millis(100))
    else {
        panic!("double click should submit the focused option");
    };
    assert_eq!(accept.request_id, RequestId::Integer(2));
    assert_eq!(accept.result, serde_json::json!({ "decision": "accept" }));
}

fn command_request(request_id: i64, command: &str) -> ServerRequest {
    ServerRequest::CommandExecutionRequestApproval {
        request_id: RequestId::Integer(request_id),
        params: CommandExecutionRequestApprovalParams {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: format!("command-{request_id}"),
            started_at_ms: 1,
            approval_id: None,
            environment_id: None,
            reason: Some("The command needs explicit approval".to_string()),
            network_approval_context: None,
            command: Some(command.to_string()),
            cwd: None,
            command_actions: None,
            additional_permissions: None,
            proposed_execpolicy_amendment: None,
            proposed_network_policy_amendments: None,
            available_decisions: None,
        },
    }
}

fn buffer_text(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|row| {
            (0..buffer.area.width)
                .map(|column| buffer[(column, row)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
