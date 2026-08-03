use codex_app_server_protocol::AgentMessageDeltaNotification;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnPlanStep;
use codex_app_server_protocol::TurnPlanStepStatus;
use codex_app_server_protocol::TurnPlanUpdatedNotification;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;
use pretty_assertions::assert_eq;

use super::ApplyOutcome;
use super::EntryLifecycle;
use super::Transcript;
use super::TranscriptGap;
use crate::LiveItem;

const THREAD_ID: &str = "thread-1";

#[test]
fn lifecycle_events_keep_stable_entries_and_authoritative_order() {
    let mut transcript = Transcript::new(THREAD_ID);
    transcript.apply(&turn_started(turn(
        "turn-1",
        Vec::new(),
        TurnStatus::InProgress,
    )));
    transcript.apply(&agent_delta("turn-1", "assistant", "early"));
    assert!(transcript.turns()[0].entries().is_empty());
    transcript.apply(&item_started("turn-1", agent("assistant", ""), 100));
    transcript.apply(&agent_delta("turn-1", "assistant", "hello"));
    transcript.apply(&item_started("turn-1", agent("second", ""), 110));
    transcript.apply(&agent_delta("turn-1", "second", "after"));
    transcript.apply(&item_completed("turn-1", agent("assistant", "hello"), 130));
    let entries = transcript.turns()[0].entries();
    assert_eq!(item_ids(&transcript, 0), vec!["assistant", "second"]);
    assert_eq!(
        entries[0].lifecycle(),
        EntryLifecycle::Completed {
            started_at_ms: Some(100),
            completed_at_ms: 130,
        }
    );
    assert_eq!(
        entries[1].live(),
        &LiveItem::AgentMessage("after".to_string())
    );
    assert_eq!(
        transcript.apply(&agent_delta("turn-1", "assistant", "late")),
        ApplyOutcome::NeedsSnapshot(TranscriptGap::ItemNotRunning)
    );
    transcript.apply(&ServerNotification::TurnCompleted(
        TurnCompletedNotification {
            thread_id: THREAD_ID.to_string(),
            turn: turn(
                "turn-1",
                vec![agent("second", "after"), agent("assistant", "hello")],
                TurnStatus::Completed,
            ),
        },
    ));
    assert_eq!(item_ids(&transcript, 0), vec!["second", "assistant"]);
}

#[test]
fn item_identity_is_scoped_by_turn() {
    let mut transcript = Transcript::new(THREAD_ID);
    for turn_id in ["turn-a", "turn-b"] {
        transcript.apply(&turn_started(turn(
            turn_id,
            Vec::new(),
            TurnStatus::InProgress,
        )));
        transcript.apply(&item_started(turn_id, agent("shared", ""), 10));
    }
    transcript.apply(&agent_delta("turn-a", "shared", "alpha"));
    transcript.apply(&agent_delta("turn-b", "shared", "beta"));

    assert_eq!(
        transcript.turns()[0].entries()[0].live(),
        &LiveItem::AgentMessage("alpha".to_string())
    );
    assert_eq!(
        transcript.turns()[1].entries()[0].live(),
        &LiveItem::AgentMessage("beta".to_string())
    );
}

#[test]
fn turn_plan_is_auxiliary_state_not_a_transcript_entry() {
    let mut transcript = Transcript::new(THREAD_ID);
    transcript.apply(&turn_started(turn(
        "turn-1",
        Vec::new(),
        TurnStatus::InProgress,
    )));
    let notification = TurnPlanUpdatedNotification {
        thread_id: THREAD_ID.to_string(),
        turn_id: "turn-1".to_string(),
        explanation: None,
        plan: vec![TurnPlanStep {
            step: "inspect".to_string(),
            status: TurnPlanStepStatus::InProgress,
        }],
    };

    transcript.apply(&ServerNotification::TurnPlanUpdated(notification.clone()));
    assert!(transcript.turns()[0].entries().is_empty());
    assert_eq!(transcript.turns()[0].plan(), Some(&notification));
}

fn item_ids(transcript: &Transcript, turn_index: usize) -> Vec<&str> {
    transcript.turns()[turn_index]
        .entries()
        .iter()
        .map(|entry| entry.item().id())
        .collect()
}

fn agent(id: &str, text: &str) -> ThreadItem {
    ThreadItem::AgentMessage {
        id: id.to_string(),
        text: text.to_string(),
        phase: None,
        memory_citation: None,
    }
}

fn turn(id: &str, items: Vec<ThreadItem>, status: TurnStatus) -> Turn {
    Turn {
        id: id.to_string(),
        items,
        items_view: TurnItemsView::Full,
        status,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }
}

fn turn_started(turn: Turn) -> ServerNotification {
    ServerNotification::TurnStarted(TurnStartedNotification {
        thread_id: THREAD_ID.to_string(),
        turn,
    })
}

fn item_started(turn_id: &str, item: ThreadItem, started_at_ms: i64) -> ServerNotification {
    ServerNotification::ItemStarted(ItemStartedNotification {
        item,
        thread_id: THREAD_ID.to_string(),
        turn_id: turn_id.to_string(),
        started_at_ms,
    })
}

fn item_completed(turn_id: &str, item: ThreadItem, completed_at_ms: i64) -> ServerNotification {
    ServerNotification::ItemCompleted(ItemCompletedNotification {
        item,
        thread_id: THREAD_ID.to_string(),
        turn_id: turn_id.to_string(),
        completed_at_ms,
    })
}

fn agent_delta(turn_id: &str, item_id: &str, delta: &str) -> ServerNotification {
    ServerNotification::AgentMessageDelta(AgentMessageDeltaNotification {
        thread_id: THREAD_ID.to_string(),
        turn_id: turn_id.to_string(),
        item_id: item_id.to_string(),
        delta: delta.to_string(),
    })
}
