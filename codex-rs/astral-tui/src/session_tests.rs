use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadNameUpdatedNotification;
use codex_app_server_protocol::UserInput;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::SessionState;
use super::turn_start_request;

#[test]
fn default_turn_request_inherits_thread_settings() {
    let request = turn_start_request(
        RequestId::Integer(9),
        "thread-1".to_string(),
        vec![UserInput::Text {
            text: "hello".to_string(),
            text_elements: Vec::new(),
        }],
    );

    let ClientRequest::TurnStart { request_id, params } = request else {
        panic!("expected turn/start");
    };
    assert_eq!(request_id, RequestId::Integer(9));
    assert_eq!(
        serde_json::to_value(params).expect("serialize params"),
        json!({
            "threadId": "thread-1",
            "clientUserMessageId": null,
            "input": [{
                "type": "text",
                "text": "hello",
                "text_elements": []
            }],
            "modelClientMetadata": null,
            "additionalContext": null,
            "environments": null,
            "cwd": null,
            "runtimeWorkspaceRoots": null,
            "approvalPolicy": null,
            "approvalsReviewer": null,
            "sandboxPolicy": null,
            "permissions": null,
            "model": null,
            "modelProvider": null,
            "effort": null,
            "summary": null,
            "personality": null,
            "outputSchema": null,
            "collaborationMode": null
        })
    );
}

#[test]
fn thread_name_notifications_update_exit_metadata() {
    let thread: Thread = serde_json::from_value(json!({
        "id": "thread-1",
        "sessionId": "session-1",
        "forkedFromId": null,
        "parentThreadId": null,
        "preview": "inspect this repo",
        "ephemeral": false,
        "modelProvider": "openai",
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
        "gitInfo": null,
        "name": null,
        "turns": []
    }))
    .expect("valid thread");
    let mut state = SessionState {
        thread,
        model: "gpt-5".to_string(),
        model_provider: "openai".to_string(),
        service_tier: None,
        active_turn_id: None,
    };

    state.observe_notification(&ServerNotification::ThreadNameUpdated(
        ThreadNameUpdatedNotification {
            thread_id: "thread-1".to_string(),
            thread_name: Some("Astral TUI".to_string()),
        },
    ));

    assert_eq!(state.thread.name.as_deref(), Some("Astral TUI"));
}
