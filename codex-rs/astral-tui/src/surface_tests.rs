use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadTokenUsage;
use codex_app_server_protocol::TokenUsageBreakdown;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use serde_json::json;

use super::SurfaceActivity;
use super::SurfaceState;
use super::TranscriptView;
use super::render_surface;
use super::render_surface_with_view;
use crate::SessionState;

fn session_state() -> SessionState {
    let thread: Thread = serde_json::from_value(json!({
        "id": "thread-1",
        "sessionId": "session-1",
        "forkedFromId": null,
        "parentThreadId": null,
        "preview": "inspect this repo",
        "ephemeral": false,
        "modelProvider": "anthropic",
        "createdAt": 1,
        "updatedAt": 2,
        "status": {"type": "idle"},
        "path": null,
        "cwd": "/workspace",
        "cliVersion": "0.0.0",
        "source": "cli",
        "threadSource": "user",
        "agentNickname": null,
        "agentRole": null,
        "gitInfo": {
            "sha": "0123456789abcdef",
            "branch": "main",
            "originUrl": "https://example.com/astral-code.git"
        },
        "name": null,
        "turns": [{
            "id": "turn-1",
            "items": [
                {
                    "type": "userMessage",
                    "id": "user-1",
                    "content": [{
                        "type": "text",
                        "text": "inspect this repo",
                        "text_elements": []
                    }]
                },
                {
                    "type": "agentMessage",
                    "id": "agent-1",
                    "text": "I’m tracing the relevant data flow.",
                    "phase": null,
                    "memoryCitation": null
                }
            ],
            "itemsView": "full",
            "status": "inProgress",
            "error": null,
            "startedAt": 1,
            "completedAt": null,
            "durationMs": null
        }]
    }))
    .expect("valid thread");
    SessionState {
        thread,
        model: "claude-sonnet-4".to_string(),
        model_provider: "anthropic".to_string(),
        service_tier: None,
        active_turn_id: Some("turn-1".to_string()),
        collaboration_mode: CollaborationMode {
            mode: ModeKind::Default,
            settings: Settings {
                model: "claude-sonnet-4".to_string(),
                reasoning_effort: None,
                developer_instructions: None,
            },
        },
    }
}

#[test]
fn working_surface_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_activity(SurfaceActivity::Working);
    state.set_token_usage(ThreadTokenUsage {
        total: TokenUsageBreakdown {
            total_tokens: 12_345,
            input_tokens: 10_000,
            cached_input_tokens: 4_000,
            output_tokens: 2_000,
            reasoning_output_tokens: 345,
        },
        last: TokenUsageBreakdown {
            total_tokens: 9_200,
            input_tokens: 8_000,
            cached_input_tokens: 4_000,
            output_tokens: 1_000,
            reasoning_output_tokens: 200,
        },
        model_context_window: Some(500_000),
    });
    state.composer_mut().push_str("follow the projection");
    let area = Rect::new(0, 0, 72, 12);
    let mut buffer = Buffer::empty(area);
    render_surface(&state, &session, area, &mut buffer);

    insta::assert_snapshot!(buffer_text(&buffer));
}

#[test]
fn grok_view_80x24_snapshot() {
    let (state, session) = named_working_surface();
    insta::assert_snapshot!(render_at_size(&state, &session, 80, 24));
}

#[test]
fn grok_view_120x32_snapshot() {
    let (state, session) = named_working_surface();
    insta::assert_snapshot!(render_at_size(&state, &session, 120, 32));
}

#[test]
fn grok_view_narrow_snapshot() {
    let (state, session) = named_working_surface();
    insta::assert_snapshot!(render_at_size(&state, &session, 48, 16));
}

#[test]
fn command_approval_surface_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    let request: ServerRequest = serde_json::from_value(json!({
        "method": "item/commandExecution/requestApproval",
        "id": 8,
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "call-1",
            "startedAtMs": 100,
            "reason": "needs network access",
            "command": "cargo test --workspace",
            "cwd": "/workspace"
        }
    }))
    .expect("valid command approval");
    state.pending_requests_mut().note(request);
    assert_eq!(
        state
            .pending_requests()
            .front()
            .map(super::super::request::PendingRequest::request_id),
        Some(&RequestId::Integer(8))
    );
    let area = Rect::new(0, 0, 72, 12);
    let mut buffer = Buffer::empty(area);
    render_surface(&state, &session, area, &mut buffer);

    insta::assert_snapshot!(buffer_text(&buffer));
}

#[test]
fn typed_approval_surfaces_snapshot() {
    insta::assert_snapshot!(
        "file_change_approval_surface",
        request_surface(
            json!({
                "method": "item/fileChange/requestApproval",
                "id": "edit-1",
                "params": {
                    "threadId": "thread-1",
                    "turnId": "turn-1",
                    "itemId": "edit-1",
                    "startedAtMs": 101,
                    "reason": "update source",
                    "grantRoot": null
                }
            }),
            ""
        )
    );
    insta::assert_snapshot!(
        "permissions_approval_surface",
        request_surface(
            json!({
                "method": "item/permissions/requestApproval",
                "id": "permissions-1",
                "params": {
                    "threadId": "thread-1",
                    "turnId": "turn-1",
                    "itemId": "call-1",
                    "environmentId": null,
                    "startedAtMs": 102,
                    "cwd": "/workspace",
                    "reason": "read generated files",
                    "permissions": {
                        "network": {"enabled": true},
                        "fileSystem": {
                            "read": ["/workspace/generated"],
                            "write": null
                        }
                    }
                }
            }),
            ""
        )
    );
    insta::assert_snapshot!(
        "mcp_form_elicitation_surface",
        request_surface(
            json!({
                "method": "mcpServer/elicitation/request",
                "id": "mcp-form",
                "params": {
                    "threadId": "thread-1",
                    "turnId": "turn-1",
                    "serverName": "astral",
                    "mode": "form",
                    "_meta": null,
                    "message": "Choose settings",
                    "requestedSchema": {
                        "type": "object",
                        "properties": {
                            "confirmed": {"type": "boolean"}
                        }
                    }
                }
            }),
            r#"{"confirmed":true}"#
        )
    );
}

#[test]
fn fullscreen_surface_keeps_committed_history_snapshot() {
    let mut session = session_state();
    session.thread.turns[0].status = codex_app_server_protocol::TurnStatus::Completed;
    session.thread.turns[0].completed_at = Some(2);
    session.active_turn_id = None;
    let mut state = SurfaceState::from_session(&session);
    assert_eq!(state.drain_committable().len(), 2);
    let area = Rect::new(0, 0, 72, 12);
    let mut buffer = Buffer::empty(area);
    render_surface_with_view(&state, &session, TranscriptView::Full, area, &mut buffer);

    insta::assert_snapshot!(buffer_text(&buffer));
}

#[test]
fn scroll_offset_moves_in_both_directions() {
    let mut state = SurfaceState::new("thread-1");
    state.scroll_up(/*lines*/ 20);
    state.scroll_down(/*lines*/ 7);
    assert_eq!(state.scroll_offset(), 13);
    state.scroll_to_bottom();
    assert_eq!(state.scroll_offset(), 0);
}

fn request_surface(value: serde_json::Value, composer: &str) -> String {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    let request: ServerRequest = serde_json::from_value(value).expect("valid server request");
    state.pending_requests_mut().note(request);
    state.composer_mut().push_str(composer);
    let area = Rect::new(0, 0, 72, 12);
    let mut buffer = Buffer::empty(area);
    render_surface(&state, &session, area, &mut buffer);
    buffer_text(&buffer)
}

fn render_at_size(state: &SurfaceState, session: &SessionState, width: u16, height: u16) -> String {
    let area = Rect::new(0, 0, width, height);
    let mut buffer = Buffer::empty(area);
    render_surface(state, session, area, &mut buffer);
    buffer_text(&buffer)
}

fn named_working_surface() -> (SurfaceState, SessionState) {
    let mut session = session_state();
    session.thread.name = Some("Astral session".to_string());
    let mut state = SurfaceState::from_session(&session);
    state.set_activity(SurfaceActivity::Working);
    state.composer_mut().push_str("trace the projection");
    (state, session)
}

fn buffer_text(buffer: &Buffer) -> String {
    let area = buffer.area;
    (area.y..area.y + area.height)
        .map(|y| {
            let mut line = String::new();
            for x in area.x..area.x + area.width {
                line.push_str(buffer[(x, y)].symbol());
            }
            line.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}
