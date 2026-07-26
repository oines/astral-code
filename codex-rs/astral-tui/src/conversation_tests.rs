use codex_app_server_protocol::CoreToolCallStatus;
use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::PatchApplyStatus;
use codex_app_server_protocol::PatchChangeKind;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use pretty_assertions::assert_eq;

use super::ConversationState;
use crate::PresentationBlock;
use crate::ToolKind;

fn completed_item(turn_id: &str, item: ThreadItem) -> ServerNotification {
    ServerNotification::ItemCompleted(ItemCompletedNotification {
        thread_id: "thread-1".to_string(),
        turn_id: turn_id.to_string(),
        item,
        completed_at_ms: 100,
    })
}

fn completed_turn(turn_id: &str) -> ServerNotification {
    ServerNotification::TurnCompleted(TurnCompletedNotification {
        thread_id: "thread-1".to_string(),
        turn: Turn {
            id: turn_id.to_string(),
            items: Vec::new(),
            items_view: TurnItemsView::Full,
            status: TurnStatus::Completed,
            error: None,
            started_at: Some(1),
            completed_at: Some(2),
            duration_ms: Some(1_000),
        },
    })
}

fn agent_message(id: &str, text: &str) -> ThreadItem {
    ThreadItem::AgentMessage {
        id: id.to_string(),
        text: text.to_string(),
        phase: None,
        memory_citation: None,
    }
}

#[test]
fn running_turn_stays_live_then_commits_after_structured_replacement() {
    let mut state = ConversationState::new("thread-1");
    state.apply(&completed_item(
        "turn-1",
        ThreadItem::CoreToolCall {
            id: "edit-1".to_string(),
            tool: "Edit".to_string(),
            arguments: serde_json::json!({"file_path": "src/lib.rs"}),
            status: CoreToolCallStatus::Completed,
            result: Some("done".to_string()),
            error: None,
            duration_ms: Some(10),
        },
    ));

    assert!(state.drain_committable().is_empty());
    assert_eq!(state.live_blocks().len(), 1);

    state.apply(&completed_item(
        "turn-1",
        ThreadItem::FileChange {
            id: "edit-1".to_string(),
            changes: vec![FileUpdateChange {
                path: "src/lib.rs".to_string(),
                kind: PatchChangeKind::Update { move_path: None },
                diff: "@@ -1 +1 @@\n-old\n+new".to_string(),
            }],
            status: PatchApplyStatus::Completed,
        },
    ));
    state.apply(&completed_turn("turn-1"));

    let committed = state.drain_committable();
    assert_eq!(committed.len(), 1);
    let PresentationBlock::Tool(tool) = &committed[0].block else {
        panic!("expected tool block");
    };
    assert_eq!(tool.kind, ToolKind::Edit);
    assert_eq!(tool.name, "edit");
    assert!(state.live_blocks().is_empty());
}

#[test]
fn later_completed_turn_does_not_jump_past_running_frontier() {
    let mut state = ConversationState::new("thread-1");
    state.apply(&completed_item(
        "turn-1",
        agent_message("message-1", "first"),
    ));
    state.apply(&completed_item(
        "turn-2",
        agent_message("message-2", "second"),
    ));
    state.apply(&completed_turn("turn-2"));

    assert!(state.drain_committable().is_empty());
    assert_eq!(state.live_blocks().len(), 2);
}
