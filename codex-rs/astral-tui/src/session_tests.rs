use codex_app_server_protocol::ActivePermissionProfile;
use codex_app_server_protocol::ApprovalsReviewer;
use codex_app_server_protocol::AskForApproval;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::SandboxPolicy;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadSettings;
use codex_app_server_protocol::ThreadSettingsUpdatedNotification;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::openai_models::ReasoningEffort;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

use super::SessionState;
use super::turn_start_request;

#[test]
fn turn_request_inherits_sticky_thread_settings() {
    let input = vec![UserInput::Text {
        text: "hello".to_string(),
        text_elements: Vec::new(),
    }];
    let request = turn_start_request(RequestId::Integer(9), "thread-1".to_string(), input.clone());

    assert_eq!(
        request,
        ClientRequest::TurnStart {
            request_id: RequestId::Integer(9),
            params: TurnStartParams {
                thread_id: "thread-1".to_string(),
                input,
                ..TurnStartParams::default()
            },
        }
    );
}

#[test]
fn session_state_observes_only_matching_lifecycle_and_settings_notifications() {
    let cwd = AbsolutePathBuf::current_dir().expect("current directory should be absolute");
    let thread = Thread {
        id: "thread-1".to_string(),
        session_id: "session-1".to_string(),
        forked_from_id: None,
        parent_thread_id: None,
        preview: "hello".to_string(),
        ephemeral: false,
        model_provider: "provider-a".to_string(),
        created_at: 1,
        updated_at: 2,
        status: ThreadStatus::Active {
            active_flags: Vec::new(),
        },
        path: None,
        cwd: cwd.clone(),
        cli_version: "0.0.0".to_string(),
        source: SessionSource::Exec,
        thread_source: None,
        agent_nickname: None,
        agent_role: None,
        git_info: None,
        name: Some("original".to_string()),
        turns: vec![
            turn("finished", TurnStatus::Completed),
            turn("active", TurnStatus::InProgress),
        ],
    };
    let mut state = SessionState::from_resume(ThreadResumeResponse {
        thread: thread.clone(),
        model: "model-a".to_string(),
        model_provider: "provider-a".to_string(),
        service_tier: None,
        cwd: cwd.clone(),
        runtime_workspace_roots: Vec::new(),
        instruction_sources: Vec::new(),
        approval_policy: AskForApproval::OnFailure,
        approvals_reviewer: ApprovalsReviewer::User,
        sandbox: SandboxPolicy::DangerFullAccess,
        active_permission_profile: None,
        reasoning_effort: Some(ReasoningEffort::Medium),
        initial_turns_page: None,
    });
    let expected_resumed = SessionState {
        thread: thread.clone(),
        model: "model-a".to_string(),
        model_provider: "provider-a".to_string(),
        service_tier: None,
        reasoning_effort: Some(ReasoningEffort::Medium),
        approval_policy: AskForApproval::OnFailure,
        approvals_reviewer: ApprovalsReviewer::User,
        active_permission_profile: None,
        active_turn_id: Some("active".to_string()),
    };
    assert_eq!(state, expected_resumed);

    state.observe_notification(&ServerNotification::TurnStarted(TurnStartedNotification {
        thread_id: "thread-2".to_string(),
        turn: turn("foreign", TurnStatus::InProgress),
    }));
    state.observe_notification(&ServerNotification::TurnCompleted(
        TurnCompletedNotification {
            thread_id: "thread-1".to_string(),
            turn: turn("stale", TurnStatus::Completed),
        },
    ));
    assert_eq!(state, expected_resumed);

    let next_cwd = cwd.join("next");
    let next_settings = ThreadSettings {
        cwd: next_cwd.clone(),
        approval_policy: AskForApproval::OnRequest,
        approvals_reviewer: ApprovalsReviewer::AutoReview,
        sandbox_policy: SandboxPolicy::DangerFullAccess,
        active_permission_profile: Some(ActivePermissionProfile::new(":read-only")),
        model: "model-b".to_string(),
        model_provider: "provider-b".to_string(),
        service_tier: Some("priority".to_string()),
        effort: Some(ReasoningEffort::High),
        summary: None,
        collaboration_mode: CollaborationMode {
            mode: ModeKind::Default,
            settings: Settings {
                model: "model-b".to_string(),
                reasoning_effort: Some(ReasoningEffort::High),
                developer_instructions: None,
            },
        },
        personality: None,
    };
    state.observe_notification(&ServerNotification::ThreadSettingsUpdated(
        ThreadSettingsUpdatedNotification {
            thread_id: "thread-2".to_string(),
            thread_settings: next_settings.clone(),
        },
    ));
    assert_eq!(state, expected_resumed);

    state.observe_notification(&ServerNotification::ThreadSettingsUpdated(
        ThreadSettingsUpdatedNotification {
            thread_id: "thread-1".to_string(),
            thread_settings: next_settings,
        },
    ));
    let mut expected_thread = thread;
    expected_thread.cwd = next_cwd;
    expected_thread.model_provider = "provider-b".to_string();
    let mut expected_updated = SessionState {
        thread: expected_thread,
        model: "model-b".to_string(),
        model_provider: "provider-b".to_string(),
        service_tier: Some("priority".to_string()),
        reasoning_effort: Some(ReasoningEffort::High),
        approval_policy: AskForApproval::OnRequest,
        approvals_reviewer: ApprovalsReviewer::AutoReview,
        active_permission_profile: Some(ActivePermissionProfile::new(":read-only")),
        active_turn_id: Some("active".to_string()),
    };
    assert_eq!(state, expected_updated);

    state.observe_notification(&ServerNotification::TurnCompleted(
        TurnCompletedNotification {
            thread_id: "thread-1".to_string(),
            turn: turn("active", TurnStatus::Completed),
        },
    ));
    expected_updated.active_turn_id = None;
    assert_eq!(state, expected_updated);
}

fn turn(id: &str, status: TurnStatus) -> Turn {
    Turn {
        id: id.to_string(),
        items: Vec::new(),
        items_view: TurnItemsView::Full,
        status,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }
}
