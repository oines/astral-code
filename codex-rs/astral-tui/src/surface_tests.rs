use codex_app_server_protocol::ActivePermissionProfile;
use codex_app_server_protocol::ApprovalsReviewer;
use codex_app_server_protocol::AskForApproval;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadTokenUsage;
use codex_app_server_protocol::TokenUsageBreakdown;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::models::BUILT_IN_PERMISSION_PROFILE_WORKSPACE;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use serde_json::json;

use super::SurfaceActivity;
use super::SurfaceState;
use super::TranscriptView;
use super::render_surface;
use super::render_surface_with_view;
use crate::SessionState;
use crate::modal::ModalRow;
use crate::modal::ModalState;
use crate::shortcuts::shortcuts_modal;
use crate::view::AstralThemeId;

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
        approval_policy: AskForApproval::OnRequest,
        approvals_reviewer: ApprovalsReviewer::User,
        active_permission_profile: Some(ActivePermissionProfile::new(
            BUILT_IN_PERMISSION_PROFILE_WORKSPACE,
        )),
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
    state.set_composer("follow the projection");
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
fn slash_command_menu_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_activity(SurfaceActivity::Ready);
    state.set_composer("/mo");

    insta::assert_snapshot!(render_at_size(&state, &session, 80, 24));
}

#[test]
fn model_argument_menu_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_activity(SurfaceActivity::Ready);
    state.set_model_catalog(
        vec![
            serde_json::from_value(json!({
                "modelProvider": "anthropic",
                "modelProviderName": "Anthropic",
                "id": "claude-sonnet-4",
                "model": "claude-sonnet-4",
                "upgrade": null,
                "upgradeInfo": null,
                "availabilityNux": null,
                "displayName": "Claude Sonnet 4",
                "description": "Fast coding model",
                "hidden": false,
                "supportedReasoningEfforts": [
                    {"reasoningEffort": "high", "description": "Deep reasoning"},
                    {"reasoningEffort": "xhigh", "description": "Maximum reasoning"}
                ],
                "defaultReasoningEffort": "high",
                "inputModalities": ["text", "image"],
                "supportsPersonality": true,
                "additionalSpeedTiers": [],
                "serviceTiers": [],
                "defaultServiceTier": null,
                "isDefault": true
            }))
            .expect("valid model"),
        ],
        session.model.clone(),
        session.model_provider.clone(),
    );
    state.set_composer("/model ");

    insta::assert_snapshot!(render_at_size(&state, &session, 80, 24));
}

#[test]
fn status_modal_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.open_modal(ModalState::info(
        "Session status",
        vec![
            ModalRow::new("Thread", "thread-1"),
            ModalRow::new("Model", "claude-sonnet-4 · anthropic"),
            ModalRow::new("Working directory", "/workspace"),
            ModalRow::new("Context", "9.2K / 500K"),
        ],
    ));

    insta::assert_snapshot!(render_at_size(&state, &session, 80, 24));
}

#[test]
fn ecosystem_modal_scroll_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    let mut modal = ModalState::info(
        "Skills",
        (0..30)
            .map(|index| {
                ModalRow::new(
                    format!("skill-{index:02}"),
                    if index % 3 == 0 {
                        "repo · enabled"
                    } else {
                        "user · enabled"
                    },
                )
            })
            .collect(),
    );
    modal.scroll_by(8);
    state.open_modal(modal);

    insta::assert_snapshot!(render_at_size(&state, &session, 80, 24));
}

#[test]
fn shortcuts_modal_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.open_modal(shortcuts_modal());

    insta::assert_snapshot!(render_at_size(&state, &session, 80, 24));
}

#[test]
fn theme_picker_surface_snapshot() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.open_theme_picker();

    insta::assert_snapshot!(render_at_size(&state, &session, 80, 24));
}

#[test]
fn timeline_rail_surface_snapshot() {
    let mut session = session_state();
    session.thread.turns.push(
        serde_json::from_value(json!({
            "id": "turn-2",
            "items": [
                {
                    "type": "userMessage",
                    "id": "user-2",
                    "content": [{
                        "type": "text",
                        "text": "continue with the implementation",
                        "text_elements": []
                    }]
                },
                {
                    "type": "agentMessage",
                    "id": "agent-2",
                    "text": "I’m validating the next layer.",
                    "phase": null,
                    "memoryCitation": null
                }
            ],
            "itemsView": "full",
            "status": "completed",
            "error": null,
            "startedAt": 3,
            "completedAt": 4,
            "durationMs": 1000
        }))
        .expect("valid second turn"),
    );
    let mut state = SurfaceState::from_session(&session);
    state.set_timeline_visible(true);

    insta::assert_snapshot!(render_at_size(&state, &session, 80, 24));
}

#[test]
fn selected_theme_controls_the_surface_background() {
    let session = session_state();
    let mut state = SurfaceState::from_session(&session);
    state.set_theme(AstralThemeId::Day);
    let area = Rect::new(0, 0, 80, 24);
    let mut buffer = Buffer::empty(area);
    render_surface(&state, &session, area, &mut buffer);

    assert_eq!(buffer[(0, 0)].bg, state.theme().bg_base);
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
    let area = Rect::new(0, 0, 72, 18);
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
    insta::assert_snapshot!(
        "user_question_surface",
        request_surface(
            json!({
                "method": "item/tool/requestUserInput",
                "id": "question-1",
                "params": {
                    "threadId": "thread-1",
                    "turnId": "turn-1",
                    "itemId": "question-1",
                    "questions": [{
                        "id": "scope",
                        "header": "Choose scope",
                        "question": "Where should the refactor apply?",
                        "isOther": false,
                        "isSecret": false,
                        "options": [{
                            "label": "Workspace only",
                            "description": "Keep the change local to this repository"
                        }, {
                            "label": "Shared runtime",
                            "description": "Update the common implementation"
                        }]
                    }]
                }
            }),
            "Workspace only"
        )
    );
    insta::assert_snapshot!(
        "mcp_url_elicitation_surface",
        request_surface(
            json!({
                "method": "mcpServer/elicitation/request",
                "id": "mcp-url",
                "params": {
                    "threadId": "thread-1",
                    "turnId": "turn-1",
                    "serverName": "linear",
                    "mode": "url",
                    "_meta": null,
                    "message": "Connect Linear to continue",
                    "url": "https://linear.app/oauth/authorize",
                    "elicitationId": "auth-1"
                }
            }),
            ""
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
    state.set_composer(composer);
    let area = Rect::new(0, 0, 72, 18);
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
    state.set_composer("trace the projection");
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
