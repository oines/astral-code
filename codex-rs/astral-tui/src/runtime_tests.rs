use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ServerRequestResolvedNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadTokenUsage;
use codex_app_server_protocol::ThreadTokenUsageUpdatedNotification;
use codex_app_server_protocol::TokenUsageBreakdown;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnError;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::config_types::ModeKind;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::RunOptions;
use super::RunViewport;
use super::configured_theme;
use super::handle_notification;
use super::plan::handle_notification as handle_plan_review_notification;
use super::viewport_rows;
use crate::PresentationBlock;
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
fn compaction_uses_started_and_completed_item_lifecycle() {
    let mut surface = SurfaceState::new("thread-1");
    let item = ThreadItem::ContextCompaction {
        id: "compact-1".to_string(),
    };

    surface.set_activity(SurfaceActivity::Compacting);
    handle_notification(
        &mut surface,
        &ServerNotification::TurnStarted(TurnStartedNotification {
            thread_id: "thread-1".to_string(),
            turn: Turn {
                id: "turn-1".to_string(),
                items: Vec::new(),
                items_view: Default::default(),
                status: TurnStatus::InProgress,
                error: None,
                started_at: Some(1),
                completed_at: None,
                duration_ms: None,
            },
        }),
    );
    assert_eq!(surface.activity(), &SurfaceActivity::Compacting);

    handle_notification(
        &mut surface,
        &ServerNotification::ItemStarted(ItemStartedNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item: item.clone(),
            started_at_ms: 10,
        }),
    );
    assert_eq!(surface.activity(), &SurfaceActivity::Compacting);
    assert!(surface.conversation().all_turns().is_empty());

    handle_notification(
        &mut surface,
        &ServerNotification::ItemCompleted(ItemCompletedNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item,
            completed_at_ms: 20,
        }),
    );
    assert_eq!(surface.activity(), &SurfaceActivity::Working);
    assert_eq!(
        surface.conversation().all_turns()[0].blocks[0].block,
        PresentationBlock::System {
            title: "Context compacted".to_string(),
            detail: None,
        }
    );

    handle_notification(&mut surface, &turn_completed(TurnStatus::Completed, None));
    assert_eq!(surface.activity(), &SurfaceActivity::Ready);
}

#[test]
fn real_plan_item_opens_review_only_after_its_live_plan_turn_completes() {
    let mut surface = SurfaceState::new("thread-1");
    let item = ServerNotification::ItemCompleted(ItemCompletedNotification {
        thread_id: "thread-1".to_string(),
        turn_id: "turn-1".to_string(),
        item: ThreadItem::Plan {
            id: "plan-1".to_string(),
            text: "# Plan\n- implement".to_string(),
        },
        completed_at_ms: 20,
    });

    handle_notification(&mut surface, &item);
    handle_plan_review_notification(&mut surface, &item, ModeKind::Plan);
    assert!(surface.plan_review().is_none());

    let completed = turn_completed(TurnStatus::Completed, None);
    handle_notification(&mut surface, &completed);
    handle_plan_review_notification(&mut surface, &completed, ModeKind::Plan);
    assert!(surface.plan_review().is_some());
}

#[test]
fn queued_follow_up_bypasses_plan_review_after_turn_completion() {
    let mut surface = SurfaceState::new("thread-1");
    surface.enqueue_follow_up(crate::PromptSubmission::text_only("refine the plan"));
    let item = ServerNotification::ItemCompleted(ItemCompletedNotification {
        thread_id: "thread-1".to_string(),
        turn_id: "turn-1".to_string(),
        item: ThreadItem::Plan {
            id: "plan-1".to_string(),
            text: "# Plan\n- implement".to_string(),
        },
        completed_at_ms: 20,
    });

    handle_plan_review_notification(&mut surface, &item, ModeKind::Plan);
    handle_plan_review_notification(
        &mut surface,
        &turn_completed(TurnStatus::Completed, None),
        ModeKind::Plan,
    );

    assert!(surface.plan_review().is_none());
    assert_eq!(surface.queued_follow_ups(), 1);
}

#[test]
fn ordinary_assistant_markdown_never_opens_plan_review() {
    let mut surface = SurfaceState::new("thread-1");
    let item = ServerNotification::ItemCompleted(ItemCompletedNotification {
        thread_id: "thread-1".to_string(),
        turn_id: "turn-1".to_string(),
        item: ThreadItem::AgentMessage {
            id: "message-1".to_string(),
            text: "# Plan\n- this is still an assistant message".to_string(),
            phase: None,
            memory_citation: None,
        },
        completed_at_ms: 20,
    });

    handle_notification(&mut surface, &item);
    handle_plan_review_notification(&mut surface, &item, ModeKind::Plan);
    let completed = turn_completed(TurnStatus::Completed, None);
    handle_notification(&mut surface, &completed);
    handle_plan_review_notification(&mut surface, &completed, ModeKind::Plan);

    assert!(surface.plan_review().is_none());
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

#[test]
fn foreign_thread_turn_notifications_do_not_change_surface_activity() {
    let mut surface = SurfaceState::new("thread-1");
    surface.set_activity(SurfaceActivity::Ready);
    handle_notification(
        &mut surface,
        &ServerNotification::TurnStarted(TurnStartedNotification {
            thread_id: "thread-2".to_string(),
            turn: Turn {
                id: "turn-2".to_string(),
                items: Vec::new(),
                items_view: Default::default(),
                status: TurnStatus::InProgress,
                error: None,
                started_at: Some(1),
                completed_at: None,
                duration_ms: None,
            },
        }),
    );
    assert_eq!(surface.activity(), &SurfaceActivity::Ready);

    surface.set_activity(SurfaceActivity::Working);
    handle_notification(
        &mut surface,
        &ServerNotification::TurnCompleted(TurnCompletedNotification {
            thread_id: "thread-2".to_string(),
            turn: Turn {
                id: "turn-2".to_string(),
                items: Vec::new(),
                items_view: Default::default(),
                status: TurnStatus::Completed,
                error: None,
                started_at: Some(1),
                completed_at: Some(2),
                duration_ms: Some(1_000),
            },
        }),
    );
    assert_eq!(surface.activity(), &SurfaceActivity::Working);
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
