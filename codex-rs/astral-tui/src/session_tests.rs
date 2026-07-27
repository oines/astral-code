use codex_app_server_protocol::ApprovalsReviewer;
use codex_app_server_protocol::AskForApproval;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::SandboxPolicy;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadNameUpdatedNotification;
use codex_app_server_protocol::ThreadSettings;
use codex_app_server_protocol::ThreadSettingsUpdatedNotification;
use codex_app_server_protocol::UserInput;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::openai_models::ReasoningEffort;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::SessionState;
use super::default_collaboration_mode;
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
        default_collaboration_mode("gpt-5".to_string(), Some(ReasoningEffort::High)),
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
            "collaborationMode": {
                "mode": "default",
                "settings": {
                    "model": "gpt-5",
                    "reasoning_effort": "high",
                    "developer_instructions": null
                }
            }
        })
    );
}

#[test]
fn thread_name_notifications_update_exit_metadata() {
    let mut state = SessionState {
        thread: thread(),
        model: "gpt-5".to_string(),
        model_provider: "openai".to_string(),
        service_tier: None,
        active_turn_id: None,
        collaboration_mode: default_collaboration_mode("gpt-5".to_string(), None),
        approval_policy: AskForApproval::OnRequest,
        approvals_reviewer: ApprovalsReviewer::User,
        active_permission_profile: None,
    };

    state.observe_notification(&ServerNotification::ThreadNameUpdated(
        ThreadNameUpdatedNotification {
            thread_id: "thread-1".to_string(),
            thread_name: Some("Astral TUI".to_string()),
        },
    ));

    assert_eq!(state.thread.name.as_deref(), Some("Astral TUI"));
    assert_eq!(state.collaboration_mode.mode, ModeKind::Default);
}

#[test]
fn thread_settings_notification_preserves_explicit_collaboration_mode() {
    let mut state = SessionState {
        thread: thread(),
        model: "gpt-5".to_string(),
        model_provider: "openai".to_string(),
        service_tier: None,
        active_turn_id: None,
        collaboration_mode: default_collaboration_mode("gpt-5".to_string(), None),
        approval_policy: AskForApproval::OnRequest,
        approvals_reviewer: ApprovalsReviewer::User,
        active_permission_profile: None,
    };

    state.observe_notification(&ServerNotification::ThreadSettingsUpdated(
        ThreadSettingsUpdatedNotification {
            thread_id: "thread-1".to_string(),
            thread_settings: ThreadSettings {
                cwd: std::path::PathBuf::from("/workspace")
                    .try_into()
                    .expect("absolute cwd"),
                approval_policy: AskForApproval::OnRequest,
                approvals_reviewer: ApprovalsReviewer::AutoReview,
                sandbox_policy: SandboxPolicy::ReadOnly {
                    network_access: false,
                },
                active_permission_profile: None,
                model: "gpt-5.2".to_string(),
                model_provider: "astral".to_string(),
                service_tier: Some("fast".to_string()),
                effort: Some(ReasoningEffort::High),
                summary: None,
                collaboration_mode: CollaborationMode {
                    mode: ModeKind::Plan,
                    settings: Settings {
                        model: "gpt-5.2".to_string(),
                        reasoning_effort: Some(ReasoningEffort::High),
                        developer_instructions: None,
                    },
                },
                personality: None,
            },
        },
    ));

    assert_eq!(
        (
            state.model,
            state.model_provider,
            state.service_tier,
            state.collaboration_mode,
            state.approval_policy,
            state.approvals_reviewer,
        ),
        (
            "gpt-5.2".to_string(),
            "astral".to_string(),
            Some("fast".to_string()),
            CollaborationMode {
                mode: ModeKind::Plan,
                settings: Settings {
                    model: "gpt-5.2".to_string(),
                    reasoning_effort: Some(ReasoningEffort::High),
                    developer_instructions: None,
                },
            },
            AskForApproval::OnRequest,
            ApprovalsReviewer::AutoReview,
        )
    );
}

fn thread() -> Thread {
    serde_json::from_value(json!({
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
    .expect("valid thread")
}
