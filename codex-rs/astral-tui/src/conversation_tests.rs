use codex_app_server_protocol::AgentMessageDeltaNotification;
use codex_app_server_protocol::CommandExecutionOutputDeltaNotification;
use codex_app_server_protocol::CoreToolCallStatus;
use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::PatchApplyStatus;
use codex_app_server_protocol::PatchChangeKind;
use codex_app_server_protocol::PlanDeltaNotification;
use codex_app_server_protocol::ReasoningSummaryTextDeltaNotification;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::TerminalInteractionNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnPlanStep;
use codex_app_server_protocol::TurnPlanStepStatus;
use codex_app_server_protocol::TurnPlanUpdatedNotification;
use codex_app_server_protocol::TurnStatus;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::ConversationState;
use super::ReduceOutcome;
use crate::PresentationBlock;
use crate::TimelineStream;
use crate::ToolKind;

fn started(turn_id: &str, item: ThreadItem) -> ServerNotification {
    ServerNotification::ItemStarted(ItemStartedNotification {
        thread_id: "thread-1".to_string(),
        turn_id: turn_id.to_string(),
        item,
        started_at_ms: 10,
    })
}

fn completed(turn_id: &str, item: ThreadItem) -> ServerNotification {
    ServerNotification::ItemCompleted(ItemCompletedNotification {
        thread_id: "thread-1".to_string(),
        turn_id: turn_id.to_string(),
        item,
        completed_at_ms: 20,
    })
}

fn completed_turn(turn_id: &str) -> ServerNotification {
    ServerNotification::TurnCompleted(TurnCompletedNotification {
        thread_id: "thread-1".to_string(),
        turn: turn(turn_id, Vec::new(), TurnStatus::Completed),
    })
}

fn turn(id: &str, items: Vec<ThreadItem>, status: TurnStatus) -> Turn {
    Turn {
        id: id.to_string(),
        items,
        items_view: TurnItemsView::Full,
        status,
        error: None,
        started_at: Some(1),
        completed_at: Some(2),
        duration_ms: Some(1_000),
    }
}

fn agent_message(id: &str, text: &str) -> ThreadItem {
    ThreadItem::AgentMessage {
        id: id.to_string(),
        text: text.to_string(),
        phase: None,
        memory_citation: None,
    }
}

fn core_tool(id: &str, tool: &str, status: CoreToolCallStatus) -> ThreadItem {
    ThreadItem::CoreToolCall {
        id: id.to_string(),
        tool: tool.to_string(),
        arguments: json!({"file_path": "src/lib.rs"}),
        status,
        result: (status == CoreToolCallStatus::Completed).then(|| "done".to_string()),
        error: None,
        duration_ms: Some(10),
    }
}

fn file_change(id: &str) -> ThreadItem {
    ThreadItem::FileChange {
        id: id.to_string(),
        changes: vec![FileUpdateChange {
            path: "src/lib.rs".to_string(),
            kind: PatchChangeKind::Update { move_path: None },
            diff: "@@ -1 +1 @@\n-old\n+new".to_string(),
        }],
        status: PatchApplyStatus::Completed,
    }
}

#[test]
fn completed_item_without_start_is_inserted() {
    let mut state = ConversationState::new("thread-1");

    assert_eq!(
        state.apply(&completed("turn-1", agent_message("message-1", "done"))),
        ReduceOutcome::Applied
    );

    let turns = state.all_turns();
    assert_eq!(turns.len(), 1);
    assert_eq!(
        turns[0].blocks[0].block,
        PresentationBlock::Assistant {
            text: "done".to_string()
        }
    );
}

#[test]
fn empty_started_assistant_stays_out_of_the_visible_transcript() {
    let mut state = ConversationState::new("thread-1");

    state.apply(&started("turn-1", agent_message("message-1", "")));

    assert!(state.all_turns().is_empty());
}

#[test]
fn delta_survives_missing_start_until_authoritative_completion() {
    let mut state = ConversationState::new("thread-1");
    state.apply(&ServerNotification::AgentMessageDelta(
        AgentMessageDeltaNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "message-1".to_string(),
            delta: "streamed".to_string(),
        },
    ));

    assert_eq!(
        state.all_turns()[0].blocks[0].block,
        PresentationBlock::Assistant {
            text: "streamed".to_string()
        }
    );

    state.apply(&started("turn-1", agent_message("message-1", "")));
    state.apply(&completed(
        "turn-1",
        agent_message("message-1", "authoritative"),
    ));

    assert_eq!(
        state.all_turns()[0].blocks[0].block,
        PresentationBlock::Assistant {
            text: "authoritative".to_string()
        }
    );
}

#[test]
fn semantic_boundaries_split_assistant_text_even_when_provider_id_is_reused() {
    let mut state = ConversationState::new("thread-1");
    state.apply(&ServerNotification::AgentMessageDelta(
        AgentMessageDeltaNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "message-1".to_string(),
            delta: "answer".to_string(),
        },
    ));
    state.apply(&completed("turn-1", agent_message("message-1", "answer")));
    state.apply(&started(
        "turn-1",
        core_tool("read-1", "Read", CoreToolCallStatus::InProgress),
    ));
    state.apply(&completed(
        "turn-1",
        core_tool("read-1", "Read", CoreToolCallStatus::Completed),
    ));
    state.apply(&ServerNotification::AgentMessageDelta(
        AgentMessageDeltaNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "message-1".to_string(),
            delta: "after tool".to_string(),
        },
    ));
    state.apply(&completed(
        "turn-1",
        agent_message("message-1", "after tool"),
    ));

    let blocks = &state.all_turns()[0].blocks;
    assert_eq!(blocks.len(), 3);
    assert_eq!(
        blocks[0].block,
        PresentationBlock::Assistant {
            text: "answer".to_string()
        }
    );
    assert!(matches!(blocks[1].block, PresentationBlock::Tool(_)));
    assert_eq!(
        blocks[2].block,
        PresentationBlock::Assistant {
            text: "after tool".to_string()
        }
    );
}

#[test]
fn replay_preserves_reused_text_ids_across_semantic_boundaries() {
    let replay = ConversationState::from_turns(
        "thread-1",
        &[turn(
            "turn-1",
            vec![
                agent_message("message-1", "before"),
                core_tool("read-1", "Read", CoreToolCallStatus::Completed),
                agent_message("message-1", "after"),
            ],
            TurnStatus::Completed,
        )],
    );

    let blocks = &replay.all_turns()[0].blocks;
    assert_eq!(blocks.len(), 3);
    assert_eq!(
        blocks[0].block,
        PresentationBlock::Assistant {
            text: "before".to_string()
        }
    );
    assert!(matches!(blocks[1].block, PresentationBlock::Tool(_)));
    assert_eq!(
        blocks[2].block,
        PresentationBlock::Assistant {
            text: "after".to_string()
        }
    );
}

#[test]
fn reasoning_boundary_prevents_later_text_from_rewriting_earlier_assistant_block() {
    let mut state = ConversationState::new("thread-1");
    state.apply(&ServerNotification::AgentMessageDelta(
        AgentMessageDeltaNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "message-1".to_string(),
            delta: "before thought".to_string(),
        },
    ));
    state.apply(&ServerNotification::ReasoningSummaryTextDelta(
        ReasoningSummaryTextDeltaNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "reasoning-1".to_string(),
            summary_index: 0,
            delta: "checking".to_string(),
        },
    ));
    state.apply(&ServerNotification::AgentMessageDelta(
        AgentMessageDeltaNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "message-1".to_string(),
            delta: "after thought".to_string(),
        },
    ));

    let blocks = &state.all_turns()[0].blocks;
    assert_eq!(blocks.len(), 3);
    assert_eq!(
        blocks[0].block,
        PresentationBlock::Assistant {
            text: "before thought".to_string()
        }
    );
    assert!(matches!(
        blocks[1].block,
        PresentationBlock::Thinking { .. }
    ));
    assert_eq!(
        blocks[2].block,
        PresentationBlock::Assistant {
            text: "after thought".to_string()
        }
    );
}

#[test]
fn completed_plan_mode_message_preserves_text_on_both_sides_of_the_plan() {
    let mut state = ConversationState::new("thread-1");
    state.apply(&ServerNotification::AgentMessageDelta(
        AgentMessageDeltaNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "message-1".to_string(),
            delta: "Preface\n".to_string(),
        },
    ));
    state.apply(&ServerNotification::PlanDelta(PlanDeltaNotification {
        thread_id: "thread-1".to_string(),
        turn_id: "turn-1".to_string(),
        item_id: "plan-1".to_string(),
        delta: "# Plan\n- implement".to_string(),
    }));
    state.apply(&ServerNotification::AgentMessageDelta(
        AgentMessageDeltaNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "message-1".to_string(),
            delta: "Post".to_string(),
        },
    ));
    state.apply(&completed(
        "turn-1",
        ThreadItem::Plan {
            id: "plan-1".to_string(),
            text: "# Plan\n- implement".to_string(),
        },
    ));
    state.apply(&ServerNotification::AgentMessageDelta(
        AgentMessageDeltaNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "message-1".to_string(),
            delta: "script".to_string(),
        },
    ));
    state.apply(&completed(
        "turn-1",
        agent_message("message-1", "Preface\nPostscript"),
    ));

    let blocks = &state.all_turns()[0].blocks;
    assert_eq!(blocks.len(), 3);
    assert_eq!(
        blocks[0].block,
        PresentationBlock::Assistant {
            text: "Preface\n".to_string()
        }
    );
    assert!(matches!(blocks[1].block, PresentationBlock::Plan { .. }));
    assert_eq!(
        blocks[2].block,
        PresentationBlock::Assistant {
            text: "Postscript".to_string()
        }
    );
    assert_eq!(state.last_agent_response(), Some("Preface\nPostscript"));
}

#[test]
fn stable_blocks_commit_before_the_turn_finishes() {
    let mut state = ConversationState::new("thread-1");
    state.apply(&completed(
        "turn-1",
        ThreadItem::UserMessage {
            id: "user-1".to_string(),
            client_id: None,
            content: Vec::new(),
        },
    ));
    state.apply(&ServerNotification::AgentMessageDelta(
        AgentMessageDeltaNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "message-1".to_string(),
            delta: "working".to_string(),
        },
    ));

    let committed = state.drain_committable();
    assert_eq!(committed.len(), 1);
    assert!(matches!(committed[0].block, PresentationBlock::User { .. }));
    assert_eq!(state.committed_entries(), 1);
    assert_eq!(state.live_turns()[0].blocks.len(), 1);
}

#[test]
fn edit_waits_for_structured_replacement_before_commit() {
    let mut state = ConversationState::new("thread-1");
    state.apply(&completed(
        "turn-1",
        core_tool("edit-1", "Edit", CoreToolCallStatus::Completed),
    ));
    assert!(state.drain_committable().is_empty());

    state.apply(&completed("turn-1", file_change("edit-1")));
    assert!(state.drain_committable().is_empty());

    state.apply(&completed_turn("turn-1"));
    let committed = state.drain_committable();
    assert_eq!(committed.len(), 1);
    let PresentationBlock::Tool(tool) = &committed[0].block else {
        panic!("expected tool block");
    };
    assert_eq!(tool.kind, ToolKind::Edit);
    assert_eq!(tool.name, "edit");
    assert!(committed[0].ends_turn);
}

#[test]
fn interleaved_notifications_keep_one_container_per_turn() {
    let mut state = ConversationState::new("thread-1");
    state.apply(&completed("turn-1", agent_message("message-1", "first")));
    state.apply(&completed("turn-2", agent_message("message-2", "second")));
    state.apply(&completed(
        "turn-1",
        core_tool("read-1", "Read", CoreToolCallStatus::Completed),
    ));

    let turns = state.all_turns();
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].id, "turn-1");
    assert_eq!(turns[0].blocks.len(), 2);
    assert_eq!(turns[1].id, "turn-2");
    assert_eq!(turns[1].blocks.len(), 1);
}

#[test]
fn later_turn_cannot_commit_past_an_unsealed_frontier() {
    let mut state = ConversationState::new("thread-1");
    state.apply(&completed("turn-1", agent_message("message-1", "first")));
    state.apply(&completed("turn-2", agent_message("message-2", "second")));
    state.apply(&completed_turn("turn-2"));

    assert!(state.drain_committable().is_empty());
    assert_eq!(state.live_turns().len(), 2);
}

#[test]
fn todo_and_plan_updates_share_one_semantic_entry_in_either_order() {
    for plan_first in [false, true] {
        let mut state = ConversationState::new("thread-1");
        let todo = completed(
            "turn-1",
            ThreadItem::CoreToolCall {
                id: "todo-1".to_string(),
                tool: "TodoWrite".to_string(),
                arguments: json!({
                    "todos": [{"content": "Inspect projection", "status": "in_progress"}]
                }),
                status: CoreToolCallStatus::Completed,
                result: Some("Todos updated".to_string()),
                error: None,
                duration_ms: Some(5),
            },
        );
        let plan = ServerNotification::TurnPlanUpdated(TurnPlanUpdatedNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            explanation: Some("Keep the UI current.".to_string()),
            plan: vec![TurnPlanStep {
                step: "Inspect projection".to_string(),
                status: TurnPlanStepStatus::Completed,
            }],
        });
        if plan_first {
            state.apply(&plan);
            state.apply(&todo);
        } else {
            state.apply(&todo);
            state.apply(&plan);
        }

        let blocks = &state.all_turns()[0].blocks;
        assert_eq!(blocks.len(), 1);
        assert!(matches!(blocks[0].block, PresentationBlock::Todo(_)));
    }
}

#[test]
fn replay_keeps_reused_and_empty_provider_ids_distinct() {
    let first = turn(
        "turn-1",
        vec![
            ThreadItem::Reasoning {
                id: String::new(),
                summary: vec!["first thought".to_string()],
                content: Vec::new(),
            },
            agent_message("message-1", "first"),
        ],
        TurnStatus::Completed,
    );
    let second = turn(
        "turn-2",
        vec![
            ThreadItem::Reasoning {
                id: String::new(),
                summary: vec!["second thought".to_string()],
                content: Vec::new(),
            },
            agent_message("message-1", "second"),
        ],
        TurnStatus::Completed,
    );

    let state = ConversationState::from_turns("thread-1", &[first, second]);

    let turns = state.all_turns();
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].blocks.len(), 2);
    assert_eq!(turns[1].blocks.len(), 2);
    assert_ne!(turns[0].blocks[0].item_id, turns[1].blocks[0].item_id);
}

#[test]
fn live_and_replay_use_the_same_semantic_projection() {
    let items = vec![
        agent_message("message-1", "answer"),
        core_tool("read-1", "Read", CoreToolCallStatus::Completed),
    ];
    let mut live = ConversationState::new("thread-1");
    for item in &items {
        live.apply(&completed("turn-1", item.clone()));
    }
    live.apply(&completed_turn("turn-1"));
    let replay =
        ConversationState::from_turns("thread-1", &[turn("turn-1", items, TurnStatus::Completed)]);

    let live_blocks = live.all_turns()[0]
        .blocks
        .iter()
        .map(|block| block.block.clone())
        .collect::<Vec<_>>();
    let replay_blocks = replay.all_turns()[0]
        .blocks
        .iter()
        .map(|block| block.block.clone())
        .collect::<Vec<_>>();
    assert_eq!(live_blocks, replay_blocks);
}

#[test]
fn background_interactions_follow_process_id() {
    let mut state = ConversationState::new("thread-1");
    let command: ThreadItem = serde_json::from_value(json!({
        "type": "commandExecution",
        "id": "command-1",
        "command": "cargo test",
        "cwd": "/workspace",
        "processId": "process-7",
        "status": "inProgress",
        "commandActions": [{"type": "unknown", "command": "cargo test"}],
        "aggregatedOutput": null,
        "exitCode": null,
        "durationMs": null
    }))
    .expect("valid background command");
    state.apply(&started("turn-1", command));
    state.apply(&ServerNotification::TerminalInteraction(
        TerminalInteractionNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "stale-item-id".to_string(),
            process_id: "process-7".to_string(),
            stdin: "continue\n".to_string(),
        },
    ));
    state.apply(&ServerNotification::CommandExecutionOutputDelta(
        CommandExecutionOutputDeltaNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "command-1".to_string(),
            delta: "done\n".to_string(),
        },
    ));

    let entry = &state.turns[0].entries[0];
    assert_eq!(
        entry.stream,
        TimelineStream::Command {
            process_id: Some("process-7".to_string()),
            output: "done\n".to_string(),
            terminal_input: vec!["continue\n".to_string()],
        }
    );
}

#[test]
fn foreign_thread_does_not_change_state() {
    let mut state = ConversationState::new("thread-1");
    let notification = ServerNotification::ItemCompleted(ItemCompletedNotification {
        thread_id: "thread-2".to_string(),
        turn_id: "turn-1".to_string(),
        item: agent_message("message-1", "wrong"),
        completed_at_ms: 20,
    });

    assert_eq!(state.apply(&notification), ReduceOutcome::DifferentThread);
    assert!(state.all_turns().is_empty());
}

#[test]
fn last_agent_response_ignores_later_non_message_items() {
    let mut state = ConversationState::new("thread-1");
    state.apply(&completed(
        "turn-1",
        agent_message("message-1", "copy this response"),
    ));
    state.apply(&completed(
        "turn-1",
        core_tool("tool-1", "Read", CoreToolCallStatus::Completed),
    ));

    assert_eq!(state.last_agent_response(), Some("copy this response"));
}
