use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ServerRequestResolvedNotification;
use codex_app_server_protocol::ThreadTokenUsage;
use codex_app_server_protocol::ThreadTokenUsageUpdatedNotification;
use codex_app_server_protocol::TokenUsageBreakdown;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnError;
use codex_app_server_protocol::TurnStatus;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::RunOptions;
use super::RunViewport;
use super::configured_theme;
use super::handle_notification;
use super::viewport_rows;
use crate::SurfaceActivity;
use crate::SurfaceState;
use crate::view::AstralThemeId;

#[test]
fn fullscreen_is_the_default_viewport() {
    assert_eq!(RunOptions::default().viewport, RunViewport::Fullscreen);
    assert_eq!(RunOptions::default().initial_theme, None);
}

#[test]
fn persisted_astral_theme_is_restored_without_accepting_classic_theme_names() {
    assert_eq!(
        configured_theme(Some("astral-day")),
        Some(AstralThemeId::Day)
    );
    assert_eq!(
        configured_theme(Some("terminal")),
        Some(AstralThemeId::Terminal)
    );
    assert_eq!(configured_theme(Some("dracula")), None);
}

#[test]
fn viewport_is_bounded_by_terminal_and_keeps_minimum_live_region() {
    assert_eq!(viewport_rows(12, 40), 12);
    assert_eq!(viewport_rows(20, 10), 9);
    assert_eq!(viewport_rows(2, 40), 5);
    assert_eq!(viewport_rows(12, 3), 3);
}

#[test]
fn token_usage_notification_updates_the_active_surface() {
    let mut surface = SurfaceState::new("thread-1");
    let token_usage = ThreadTokenUsage {
        total: usage(12_345),
        last: usage(9_200),
        model_context_window: Some(500_000),
    };
    handle_notification(
        &mut surface,
        &ServerNotification::ThreadTokenUsageUpdated(ThreadTokenUsageUpdatedNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            token_usage: token_usage.clone(),
        }),
    );

    assert_eq!(surface.token_usage(), Some(&token_usage));
}

#[test]
fn turn_completion_preserves_terminal_activity_states() {
    let mut surface = SurfaceState::new("thread-1");

    handle_notification(&mut surface, &turn_completed(TurnStatus::Interrupted, None));
    assert_eq!(surface.activity(), &SurfaceActivity::Interrupted);

    handle_notification(
        &mut surface,
        &turn_completed(
            TurnStatus::Failed,
            Some(TurnError {
                message: "tool failed".to_string(),
                codex_error_info: None,
                additional_details: None,
            }),
        ),
    );
    assert_eq!(surface.activity(), &SurfaceActivity::Ready);

    handle_notification(&mut surface, &turn_completed(TurnStatus::Completed, None));
    assert_eq!(surface.activity(), &SurfaceActivity::Ready);
}

#[test]
fn resolved_notifications_only_clear_the_matching_thread_request() {
    let mut surface = SurfaceState::new("thread-1");
    let request: ServerRequest = serde_json::from_value(json!({
        "method": "item/tool/requestUserInput",
        "id": "question-1",
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "call-1",
            "questions": [{
                "id": "answer",
                "header": "Answer",
                "question": "Continue?",
                "options": null
            }]
        }
    }))
    .expect("valid user input request");
    let ServerRequest::ToolRequestUserInput { params, .. } = &request else {
        panic!("expected user input request");
    };
    let params = params.clone();
    surface.pending_requests_mut().note(request);
    surface.sync_request_states();
    assert!(
        surface
            .request_user_input_mut()
            .handle_paste(&params, "draft")
    );

    handle_notification(
        &mut surface,
        &ServerNotification::ServerRequestResolved(ServerRequestResolvedNotification {
            thread_id: "thread-2".to_string(),
            request_id: RequestId::String("question-1".to_string()),
        }),
    );
    assert_eq!(surface.pending_requests().len(), 1);
    assert_eq!(surface.request_user_input().editor(), "draft");

    handle_notification(
        &mut surface,
        &ServerNotification::ServerRequestResolved(ServerRequestResolvedNotification {
            thread_id: "thread-1".to_string(),
            request_id: RequestId::String("question-1".to_string()),
        }),
    );
    assert!(surface.pending_requests().is_empty());
    assert!(surface.request_user_input().editor().is_empty());
}

fn turn_completed(status: TurnStatus, error: Option<TurnError>) -> ServerNotification {
    ServerNotification::TurnCompleted(TurnCompletedNotification {
        thread_id: "thread-1".to_string(),
        turn: Turn {
            id: "turn-1".to_string(),
            items: Vec::new(),
            items_view: Default::default(),
            status,
            error,
            started_at: Some(1),
            completed_at: Some(2),
            duration_ms: Some(1_000),
        },
    })
}

fn usage(total_tokens: i64) -> TokenUsageBreakdown {
    TokenUsageBreakdown {
        total_tokens,
        input_tokens: total_tokens,
        cached_input_tokens: 0,
        output_tokens: 0,
        reasoning_output_tokens: 0,
    }
}
