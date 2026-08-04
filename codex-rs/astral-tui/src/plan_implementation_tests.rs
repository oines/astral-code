use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnPlanUpdatedNotification;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::FRESH_CONTEXT_PREFIX;
use super::PlanImplementationHost;
use super::PlanImplementationOutcome;
use super::PlanImplementationRequest;
use super::PlanImplementationSelection;
use super::PlanImplementationTracker;

#[test]
fn completed_plan_arms_once_and_non_plan_or_failed_turns_do_not() {
    let mut tracker = PlanImplementationTracker::default();
    tracker.observe_event(
        "thread-1",
        &notification(ServerNotification::TurnPlanUpdated(
            TurnPlanUpdatedNotification {
                thread_id: "thread-1".to_string(),
                turn_id: "turn-1".to_string(),
                explanation: None,
                plan: Vec::new(),
            },
        )),
    );
    assert_eq!(tracker.request(), None);

    tracker.observe_event(
        "thread-1",
        &notification(ServerNotification::ItemCompleted(
            ItemCompletedNotification {
                item: ThreadItem::Plan {
                    id: "plan-1".to_string(),
                    text: "# Ship it\n- inspect\n- implement".to_string(),
                },
                thread_id: "thread-1".to_string(),
                turn_id: "turn-1".to_string(),
                completed_at_ms: 10,
            },
        )),
    );
    assert_eq!(tracker.request(), None);
    tracker.observe_event(
        "thread-1",
        &notification(ServerNotification::TurnCompleted(
            TurnCompletedNotification {
                thread_id: "thread-1".to_string(),
                turn: turn("turn-1", TurnStatus::Completed),
            },
        )),
    );
    assert_eq!(
        tracker.request(),
        Some(&PlanImplementationRequest {
            turn_id: "turn-1".to_string(),
            item_id: "plan-1".to_string(),
            plan_markdown: "# Ship it\n- inspect\n- implement".to_string(),
        })
    );

    tracker.observe_event(
        "thread-1",
        &notification(ServerNotification::TurnStarted(TurnStartedNotification {
            thread_id: "thread-1".to_string(),
            turn: turn("turn-2", TurnStatus::InProgress),
        })),
    );
    tracker.observe_event(
        "thread-1",
        &notification(ServerNotification::ItemCompleted(
            ItemCompletedNotification {
                item: ThreadItem::Plan {
                    id: "plan-2".to_string(),
                    text: "# Interrupted".to_string(),
                },
                thread_id: "thread-1".to_string(),
                turn_id: "turn-2".to_string(),
                completed_at_ms: 20,
            },
        )),
    );
    tracker.observe_event(
        "thread-1",
        &notification(ServerNotification::TurnCompleted(
            TurnCompletedNotification {
                thread_id: "thread-1".to_string(),
                turn: turn("turn-2", TurnStatus::Failed),
            },
        )),
    );
    assert_eq!(tracker.request(), None);
}

#[test]
fn prompt_renders_original_choices_and_returns_exact_actions() {
    let request = PlanImplementationRequest {
        turn_id: "turn-1".to_string(),
        item_id: "plan-1".to_string(),
        plan_markdown: "# Final plan\n\n- inspect\n- implement".to_string(),
    };
    let mut host = PlanImplementationHost::new();
    assert!(host.sync(Some(&request)));
    let area = Rect::new(0, 0, 76, host.desired_height(76, 14));
    let mut buffer = Buffer::empty(area);
    host.render(&mut buffer, area);
    insta::assert_snapshot!(buffer_text(&buffer));

    assert_eq!(
        host.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        PlanImplementationOutcome::Selected(PlanImplementationSelection::ImplementCurrentThread {
            input: "Implement the plan.".to_string(),
        })
    );
    assert_eq!(
        host.handle_key_event(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)),
        PlanImplementationOutcome::Selected(PlanImplementationSelection::ImplementFreshThread {
            input: format!("{FRESH_CONTEXT_PREFIX}\n\n# Final plan\n\n- inspect\n- implement"),
        })
    );
    assert_eq!(
        host.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        PlanImplementationOutcome::Selected(PlanImplementationSelection::StayInPlanMode)
    );
}

fn notification(notification: ServerNotification) -> AppServerEvent {
    AppServerEvent::ServerNotification(notification)
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
