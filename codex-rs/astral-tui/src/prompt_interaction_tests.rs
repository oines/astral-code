use std::time::Duration;
use std::time::Instant;

use codex_app_server_protocol::CommandExecutionRequestApprovalParams;
use codex_app_server_protocol::McpServerElicitationRequest;
use codex_app_server_protocol::McpServerElicitationRequestParams;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ToolRequestUserInputOption;
use codex_app_server_protocol::ToolRequestUserInputParams;
use codex_app_server_protocol::ToolRequestUserInputQuestion;
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
use super::mcp_form::McpFormPrompt;
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

#[test]
fn ask_user_preserves_question_semantics_and_masks_secret_notes() {
    let question = |id: &str, header: &str, options: Option<Vec<ToolRequestUserInputOption>>| {
        ToolRequestUserInputQuestion {
            id: id.to_string(),
            header: header.to_string(),
            question: format!("Choose {header}"),
            is_other: options.is_some(),
            is_secret: options.is_none(),
            options,
        }
    };
    let option = |label: &str, description: &str| ToolRequestUserInputOption {
        label: label.to_string(),
        description: description.to_string(),
    };
    let mut pending = PendingInteractions::new("thread-1");
    pending.observe_request(ServerRequest::ToolRequestUserInput {
        request_id: RequestId::Integer(7),
        params: ToolRequestUserInputParams {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "questions-1".to_string(),
            questions: vec![
                question(
                    "scope",
                    "Scope",
                    Some(vec![
                        option("Alpha", "Change the first target"),
                        option("Beta", "Change the second target"),
                    ]),
                ),
                question("token", "Token", None),
            ],
        },
    });
    let mut host = PromptInteractionHost::new();
    assert!(host.sync(&pending));
    let area = Rect::new(0, 0, 72, host.desired_height(72, 14));
    let mut buffer = Buffer::empty(area);
    host.render(&mut buffer, area);
    insta::assert_snapshot!(buffer_text(&buffer));

    host.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    host.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    host.handle_paste("s3cr3t");
    let mut secret_buffer = Buffer::empty(area);
    host.render(&mut secret_buffer, area);
    assert!(!buffer_text(&secret_buffer).contains("s3cr3t"));
    let PromptInteractionOutcome::Submit(submission) =
        host.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
    else {
        panic!("last question should submit");
    };
    assert_eq!(submission.request_id, RequestId::Integer(7));
    assert_eq!(
        submission.result,
        serde_json::json!({"answers": {
            "scope": {"answers": ["Beta"]},
            "token": {"answers": ["user_note: s3cr3t"]}
        }})
    );
}

#[test]
fn mcp_url_elicitation_opens_only_safe_links_and_preserves_two_stage_flow() {
    let mut pending = PendingInteractions::new("thread-1");
    pending.observe_request(mcp_url_request(8, "https://payments.example/checkout/123"));
    let mut host = PromptInteractionHost::new();
    assert!(host.sync(&pending));
    let area = Rect::new(0, 0, 72, host.desired_height(72, 16));
    let mut buffer = Buffer::empty(area);
    host.render(&mut buffer, area);
    insta::assert_snapshot!(buffer_text(&buffer));

    assert_eq!(
        host.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        PromptInteractionOutcome::OpenExternalUrl {
            url: "https://payments.example/checkout/123".to_string()
        }
    );
    let waiting_area = Rect::new(0, 0, 72, host.desired_height(72, 16));
    let mut waiting_buffer = Buffer::empty(waiting_area);
    host.render(&mut waiting_buffer, waiting_area);
    insta::assert_snapshot!(
        "mcp_url_elicitation_waits_for_browser_confirmation",
        buffer_text(&waiting_buffer)
    );
    assert_eq!(
        host.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        PromptInteractionOutcome::Changed
    );
    assert_eq!(
        host.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        PromptInteractionOutcome::Changed
    );
    assert_eq!(
        host.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        PromptInteractionOutcome::OpenExternalUrl {
            url: "https://payments.example/checkout/123".to_string()
        }
    );
    let PromptInteractionOutcome::Submit(accepted) =
        host.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
    else {
        panic!("finishing the browser action should submit");
    };
    assert_eq!(accepted.request_id, RequestId::Integer(8));
    assert_eq!(
        accepted.result,
        serde_json::json!({
            "action": "accept", "content": null, "_meta": null
        })
    );
}

#[test]
fn mcp_url_elicitation_blocks_every_unsafe_url_shape() {
    let unsafe_urls = [
        "http://example.com/action",
        "not a URL",
        "https://user@example.com/action",
        "https://user:pass@example.com/action",
    ];

    for (offset, url) in unsafe_urls.into_iter().enumerate() {
        let request_id = 9 + offset as i64;
        let mut pending = PendingInteractions::new("thread-1");
        pending.observe_request(mcp_url_request(request_id, url));
        let mut host = PromptInteractionHost::new();
        assert!(host.sync(&pending));

        if offset == 0 {
            let area = Rect::new(0, 0, 72, host.desired_height(72, 16));
            let mut buffer = Buffer::empty(area);
            host.render(&mut buffer, area);
            insta::assert_snapshot!(
                "mcp_url_elicitation_blocks_unsafe_link",
                buffer_text(&buffer)
            );
        }

        let PromptInteractionOutcome::Submit(declined) =
            host.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            panic!("unsafe URL {url:?} should offer only decline");
        };
        assert_eq!(declined.request_id, RequestId::Integer(request_id));
        assert_eq!(
            declined.result,
            serde_json::json!({
                "action": "decline", "content": null, "_meta": null
            })
        );
    }
}

#[test]
fn mcp_form_elicitation_omits_unselected_optional_field() {
    let mut pending = PendingInteractions::new("thread-1");
    pending.observe_request(mcp_form_request(13));
    let mut host = PromptInteractionHost::new();
    assert!(host.sync(&pending));

    let area = Rect::new(0, 0, 76, host.desired_height(76, 16));
    let mut buffer = Buffer::empty(area);
    host.render(&mut buffer, area);
    insta::assert_snapshot!(buffer_text(&buffer));

    assert!(matches!(
        host.handle_key_event(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)),
        PromptInteractionOutcome::Submit(submission)
            if submission.result == serde_json::json!({"action":"decline","content":null,"_meta":null})
    ));
    assert_eq!(
        host.handle_paste("Astral"),
        PromptInteractionOutcome::Changed
    );
    assert_eq!(
        host.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        PromptInteractionOutcome::Changed
    );
    let PromptInteractionOutcome::Submit(submission) =
        host.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
    else {
        panic!("the final field should submit the form");
    };
    assert_eq!(submission.request_id, RequestId::Integer(13));
    assert_eq!(
        submission.result,
        serde_json::json!({
            "action": "accept",
            "content": {"a_project": "Astral"},
            "_meta": null
        })
    );
}

#[test]
fn mcp_tool_action_form_preserves_parameters_and_persist_choice() {
    let request = mcp_action_request(
        14,
        "Allow Calendar to create an event?",
        serde_json::json!({
            "codex_approval_kind": "mcp_tool_call",
            "tool_name": "create_event",
            "persist": ["session", "always"],
            "tool_params_display": [
                {
                    "name": "title",
                    "display_name": "Title",
                    "value": "Roadmap review with a deliberately long description that must remain bounded inside the approval modal"
                },
                {"name": "calendar", "display_name": "Calendar", "value": "Astral"},
                {"name": "timezone", "display_name": "Timezone", "value": "Asia/Singapore"},
                {"name": "notify", "display_name": "Notify", "value": true}
            ]
        }),
    );
    assert!(McpFormPrompt::from_request(&request).is_none());
    let mut pending = PendingInteractions::new("thread-1");
    pending.observe_request(request);
    let mut host = PromptInteractionHost::new();
    assert!(host.sync(&pending));
    assert!(host.is_presentable());

    let area = Rect::new(0, 0, 76, host.desired_height(76, 16));
    let mut buffer = Buffer::empty(area);
    host.render(&mut buffer, area);
    insta::assert_snapshot!(buffer_text(&buffer));

    assert_eq!(
        host.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        PromptInteractionOutcome::Changed
    );
    assert_eq!(
        host.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        PromptInteractionOutcome::Changed
    );
    let PromptInteractionOutcome::Submit(submission) =
        host.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
    else {
        panic!("persistent approval should submit");
    };
    assert_eq!(submission.request_id, RequestId::Integer(14));
    assert_eq!(
        submission.result,
        serde_json::json!({
            "action": "accept",
            "content": null,
            "_meta": {"persist": "always"}
        })
    );
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

fn mcp_url_request(request_id: i64, url: &str) -> ServerRequest {
    ServerRequest::McpServerElicitationRequest {
        request_id: RequestId::Integer(request_id),
        params: McpServerElicitationRequestParams {
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            server_name: "payments".to_string(),
            request: McpServerElicitationRequest::Url {
                meta: None,
                message: "Review the payment details to continue.".to_string(),
                url: url.to_string(),
                elicitation_id: "payment-123".to_string(),
            },
        },
    }
}

fn mcp_form_request(request_id: i64) -> ServerRequest {
    ServerRequest::McpServerElicitationRequest {
        request_id: RequestId::Integer(request_id),
        params: McpServerElicitationRequestParams {
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            server_name: "workspace-tools".to_string(),
            request: McpServerElicitationRequest::Form {
                meta: None,
                message: "Configure the generated workspace.".to_string(),
                requested_schema: serde_json::from_value(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "a_project": {
                            "type": "string",
                            "title": "Project",
                            "description": "Name shown in the generated workspace.",
                            "minLength": 2
                        },
                        "b_tier": {
                            "type": "string",
                            "title": "Tier",
                            "oneOf": [
                                {"const": "flash", "title": "Flash"},
                                {"const": "pro", "title": "Pro"}
                            ]
                        }
                    },
                    "required": ["a_project"]
                }))
                .expect("form schema should deserialize"),
            },
        },
    }
}

fn mcp_action_request(request_id: i64, message: &str, meta: serde_json::Value) -> ServerRequest {
    ServerRequest::McpServerElicitationRequest {
        request_id: RequestId::Integer(request_id),
        params: McpServerElicitationRequestParams {
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            server_name: "codex-apps".to_string(),
            request: McpServerElicitationRequest::Form {
                meta: Some(meta),
                message: message.to_string(),
                requested_schema: serde_json::from_value(serde_json::json!({
                    "type": "object",
                    "properties": {}
                }))
                .expect("empty action schema should deserialize"),
            },
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
