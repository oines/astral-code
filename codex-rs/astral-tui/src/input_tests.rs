use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ThreadListResponse;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::InputAction;
use super::handle_key;
use super::handle_paste;
use crate::RequestResolution;
use crate::SlashCommandId;
use crate::SlashInvocation;
use crate::SurfaceActivity;
use crate::SurfaceState;
use crate::modal::ModalRow;
use crate::modal::ModalState;
use crate::thread_picker::PickerState;
use crate::thread_picker::ThreadPickerAction;
use crate::view::AstralThemeId;

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
        InputAction::Submit(crate::PromptSubmission {
            text: "hello".to_string(),
            mentions: Vec::new(),
        })
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
fn transcript_shortcuts_are_distinct_from_composer_input() {
    let mut state = SurfaceState::new("thread-1");

    assert_eq!(
        handle_key(&mut state, key(KeyCode::PageUp)),
        InputAction::ScrollUp
    );
    assert_eq!(
        handle_key(&mut state, key(KeyCode::PageDown)),
        InputAction::ScrollDown
    );
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
        ),
        InputAction::CopyLastResponse
    );
    assert!(state.composer().is_empty());
}

#[test]
fn shift_tab_cycles_mode_without_consuming_the_draft() {
    for key in [
        KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE),
        KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
        KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT),
    ] {
        let mut state = SurfaceState::new("thread-1");
        state.set_composer("keep this draft");
        assert_eq!(handle_key(&mut state, key), InputAction::CycleMode);
        assert_eq!(state.composer(), "keep this draft");
    }
}

#[test]
fn slash_completion_and_dispatch_stay_local_to_the_tui() {
    let mut state = SurfaceState::new("thread-1");
    for character in "/co".chars() {
        assert_eq!(
            handle_key(&mut state, key(KeyCode::Char(character))),
            InputAction::Redraw
        );
    }
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Redraw
    );
    assert_eq!(state.composer(), "/compact");
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Slash(SlashInvocation {
            command: SlashCommandId::Compact,
            name: "compact",
            args: String::new(),
        })
    );
    assert!(state.composer().is_empty());
}

#[test]
fn slash_errors_stay_local_to_the_tui() {
    let mut state = SurfaceState::new("thread-1");
    state.set_composer("/does-not-exist");
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Notice("Unknown command: /does-not-exist".to_string())
    );
    assert_eq!(state.composer(), "/does-not-exist");

    state.set_composer("/model");
    state.set_activity(SurfaceActivity::Working);
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Notice("/model is unavailable while Astral is working".to_string())
    );

    state.set_composer("/compact");
    state.set_activity(SurfaceActivity::Disconnected("connection lost".to_string()));
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Notice("/compact requires an app-server connection".to_string())
    );
}

#[test]
fn modal_focus_blocks_composer_input_until_escape() {
    let mut state = SurfaceState::new("thread-1");
    state.open_modal(ModalState::info(
        "Session status",
        vec![ModalRow::new("Model", "gpt-5")],
    ));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('x'))),
        InputAction::None
    );
    assert!(state.composer().is_empty());
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Esc)),
        InputAction::Redraw
    );
    assert!(state.modal().is_none());
}

#[test]
fn modal_inventory_scrolls_without_touching_the_composer() {
    let mut state = SurfaceState::new("thread-1");
    state.open_modal(ModalState::info(
        "Skills",
        (0..20)
            .map(|index| ModalRow::new(format!("Skill {index}"), "enabled"))
            .collect(),
    ));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::PageDown)),
        InputAction::Redraw
    );
    assert_eq!(state.modal().map(|modal| modal.scroll_offset), Some(10));
    assert_eq!(
        handle_key(&mut state, key(KeyCode::End)),
        InputAction::Redraw
    );
    assert_eq!(state.modal().map(|modal| modal.scroll_offset), Some(19));
    assert!(state.composer().is_empty());
}

#[test]
fn shortcuts_toggle_is_global() {
    let mut state = SurfaceState::new("thread-1");
    assert_eq!(
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('.'), KeyModifiers::CONTROL)
        ),
        InputAction::OpenShortcuts
    );
}

#[test]
fn theme_cancel_restores_the_original_preview() {
    let mut state = SurfaceState::new("thread-1");
    state.open_theme_picker();
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Down)),
        InputAction::Redraw
    );
    assert_eq!(state.theme_id(), AstralThemeId::Day);
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Esc)),
        InputAction::Redraw
    );
    assert_eq!(state.theme_id(), AstralThemeId::Night);
    assert!(state.theme_picker().is_none());
}

#[test]
fn theme_selection_reports_the_persisted_name() {
    let mut state = SurfaceState::new("thread-1");
    state.open_theme_picker();
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Down)),
        InputAction::Redraw
    );
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::SelectTheme("astral-day".to_string())
    );
    assert_eq!(state.theme_id(), AstralThemeId::Day);
    assert!(state.theme_picker().is_none());
}

#[test]
fn thread_picker_owns_text_input_until_escape() {
    let mut state = SurfaceState::new("thread-1");
    state.open_thread_picker(PickerState::new(
        ThreadPickerAction::Resume,
        ThreadListResponse {
            data: Vec::new(),
            next_cursor: None,
            backwards_cursor: None,
        },
    ));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('x'))),
        InputAction::Redraw
    );
    assert_eq!(handle_paste(&mut state, "yz"), InputAction::Redraw);
    assert!(state.thread_picker().is_some());
    assert!(state.composer().is_empty());
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Esc)),
        InputAction::Redraw
    );
    assert!(state.thread_picker().is_none());
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
fn command_approval_respects_available_decisions() {
    let mut state = SurfaceState::new("thread-1");
    state.pending_requests_mut().note(request(json!({
        "method": "item/commandExecution/requestApproval",
        "id": 8,
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "call-1",
            "startedAtMs": 100,
            "availableDecisions": ["accept", "decline"]
        }
    })));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('a'))),
        InputAction::None
    );
    assert_eq!(state.pending_requests().len(), 1);
}

#[test]
fn file_change_and_permission_approvals_preserve_scope() {
    let mut state = SurfaceState::new("thread-1");
    state.pending_requests_mut().note(request(json!({
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
    })));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('a'))),
        InputAction::Resolve(RequestResolution::Success {
            request_id: RequestId::String("edit-1".to_string()),
            result: json!({"decision": "acceptForSession"}),
        })
    );

    state.pending_requests_mut().note(request(json!({
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
    })));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('a'))),
        InputAction::Resolve(RequestResolution::Success {
            request_id: RequestId::String("permissions-1".to_string()),
            result: json!({
                "permissions": {
                    "network": {"enabled": true},
                    "fileSystem": {
                        "read": ["/workspace/generated"],
                        "write": null
                    }
                },
                "scope": "session"
            }),
        })
    );
}

#[test]
fn declining_permissions_rejects_the_request() {
    let mut state = SurfaceState::new("thread-1");
    state.pending_requests_mut().note(request(json!({
        "method": "item/permissions/requestApproval",
        "id": 9,
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "call-1",
            "environmentId": null,
            "startedAtMs": 102,
            "cwd": "/workspace",
            "reason": null,
            "permissions": {
                "network": null,
                "fileSystem": null
            }
        }
    })));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('n'))),
        InputAction::Resolve(RequestResolution::Reject {
            request_id: RequestId::Integer(9),
            error: codex_app_server_protocol::JSONRPCErrorError {
                code: -32000,
                message: "permission request declined".to_string(),
                data: None,
            },
        })
    );
}

#[test]
fn mcp_form_and_url_elicitations_keep_typed_actions() {
    let mut state = SurfaceState::new("thread-1");
    state.pending_requests_mut().note(request(json!({
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
    })));
    state.set_composer(r#"{"confirmed":true}"#);

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        InputAction::Resolve(RequestResolution::Success {
            request_id: RequestId::String("mcp-form".to_string()),
            result: json!({
                "action": "accept",
                "content": {"confirmed": true},
                "_meta": null
            }),
        })
    );

    state.pending_requests_mut().note(request(json!({
        "method": "mcpServer/elicitation/request",
        "id": "mcp-url",
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "serverName": "astral",
            "mode": "url",
            "_meta": null,
            "message": "Open authorization page",
            "url": "https://example.com/auth",
            "elicitationId": "elicit-1"
        }
    })));

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('y'))),
        InputAction::Resolve(RequestResolution::Success {
            request_id: RequestId::String("mcp-url".to_string()),
            result: json!({
                "action": "accept",
                "content": null,
                "_meta": null
            }),
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
    state.set_composer("Rust | concise");

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
