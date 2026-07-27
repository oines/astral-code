use codex_app_server_protocol::AgentMessageDeltaNotification;
use codex_app_server_protocol::CommandExecutionOutputDeltaNotification;
use codex_app_server_protocol::CoreToolCallStatus;
use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::PatchApplyStatus;
use codex_app_server_protocol::PatchChangeKind;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::TerminalInteractionNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::TurnPlanStep;
use codex_app_server_protocol::TurnPlanStepStatus;
use codex_app_server_protocol::TurnPlanUpdatedNotification;
use codex_app_server_protocol::build_turns_from_rollout_items;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::TranscriptItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::TurnCompleteEvent;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::protocol::UserMessageEvent;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::ReduceOutcome;
use super::TimelineState;
use astral_tui_scrollback::PresentationBlock;
use astral_tui_scrollback::TimelineStream;
use astral_tui_scrollback::TodoItemPresentation;
use astral_tui_scrollback::TodoPresentation;
use astral_tui_scrollback::TodoStatus;

fn agent_message(id: &str, text: &str) -> ThreadItem {
    ThreadItem::AgentMessage {
        id: id.to_owned(),
        text: text.to_owned(),
        phase: None,
        memory_citation: None,
    }
}

fn background_command(id: &str, process_id: &str) -> ThreadItem {
    serde_json::from_value(json!({
        "type": "commandExecution",
        "id": id,
        "command": "cargo test",
        "cwd": "/workspace",
        "processId": process_id,
        "status": "inProgress",
        "commandActions": [{"type": "unknown", "command": "cargo test"}],
        "aggregatedOutput": null,
        "exitCode": null,
        "durationMs": null
    }))
    .expect("valid background command")
}

fn started(thread_id: &str, turn_id: &str, item: ThreadItem) -> ServerNotification {
    ServerNotification::ItemStarted(ItemStartedNotification {
        item,
        thread_id: thread_id.to_owned(),
        turn_id: turn_id.to_owned(),
        started_at_ms: 10,
    })
}

fn completed(thread_id: &str, turn_id: &str, item: ThreadItem) -> ServerNotification {
    ServerNotification::ItemCompleted(ItemCompletedNotification {
        item,
        thread_id: thread_id.to_owned(),
        turn_id: turn_id.to_owned(),
        completed_at_ms: 20,
    })
}

#[test]
fn completed_item_without_start_is_inserted() {
    let mut timeline = TimelineState::new("thread-1");

    let outcome = timeline.apply(&completed(
        "thread-1",
        "turn-1",
        agent_message("message-1", "done"),
    ));

    assert_eq!(outcome, ReduceOutcome::Applied);
    assert_eq!(timeline.entries().len(), 1);
    assert_eq!(
        timeline.entries()[0].item(),
        Some(&agent_message("message-1", "done"))
    );
    assert_eq!(timeline.entries()[0].completed_at_ms(), Some(20));
}

#[test]
fn delta_survives_missing_start_until_authoritative_completion() {
    let mut timeline = TimelineState::new("thread-1");
    let delta = ServerNotification::AgentMessageDelta(AgentMessageDeltaNotification {
        thread_id: "thread-1".to_owned(),
        turn_id: "turn-1".to_owned(),
        item_id: "message-1".to_owned(),
        delta: "streamed".to_owned(),
    });

    assert_eq!(timeline.apply(&delta), ReduceOutcome::Applied);
    assert_eq!(
        timeline.entries()[0].stream(),
        &TimelineStream::AgentMessage("streamed".to_owned())
    );
    assert_eq!(
        timeline.entries()[0].effective_agent_message(),
        Some("streamed".to_owned())
    );

    timeline.apply(&started(
        "thread-1",
        "turn-1",
        agent_message("message-1", ""),
    ));
    assert_eq!(
        timeline.entries()[0].effective_agent_message(),
        Some("streamed".to_owned())
    );

    timeline.apply(&completed(
        "thread-1",
        "turn-1",
        agent_message("message-1", "authoritative"),
    ));
    assert_eq!(
        timeline.entries()[0].effective_agent_message(),
        Some("authoritative".to_owned())
    );
    assert_eq!(timeline.entries()[0].stream(), &TimelineStream::None);
}

#[test]
fn structured_file_change_replaces_matching_core_tool_call() {
    let mut timeline = TimelineState::new("thread-1");
    let core_tool = ThreadItem::CoreToolCall {
        id: "call-1".to_owned(),
        tool: "Edit".to_owned(),
        arguments: Default::default(),
        status: CoreToolCallStatus::Completed,
        result: Some("edited".to_owned()),
        error: None,
        duration_ms: Some(5),
    };
    let file_change = ThreadItem::FileChange {
        id: "call-1".to_owned(),
        changes: vec![FileUpdateChange {
            path: "/tmp/example.rs".to_owned(),
            kind: PatchChangeKind::Update { move_path: None },
            diff: "@@ -1 +1 @@\n-old\n+new".to_owned(),
        }],
        status: PatchApplyStatus::Completed,
    };

    timeline.apply(&completed("thread-1", "turn-1", core_tool));
    timeline.apply(&completed("thread-1", "turn-1", file_change.clone()));

    assert_eq!(timeline.entries().len(), 1);
    assert_eq!(timeline.entries()[0].item(), Some(&file_change));
}

#[test]
fn foreign_thread_notification_is_ignored_without_state_change() {
    let mut timeline = TimelineState::new("thread-1");

    let outcome = timeline.apply(&completed(
        "thread-2",
        "turn-1",
        agent_message("message-1", "wrong thread"),
    ));

    assert_eq!(outcome, ReduceOutcome::DifferentThread);
    assert_eq!(timeline.entries(), &[]);
}

#[test]
fn replay_scopes_reused_item_ids_to_their_turn() {
    let mut timeline = TimelineState::new("thread-1");
    let first = agent_message("message-1", "first");
    let replacement = agent_message("message-1", "replacement");
    let second = agent_message("message-2", "second");

    timeline.replace_from_turns([
        ("turn-1", std::slice::from_ref(&first)),
        ("turn-2", &[replacement.clone(), second.clone()]),
    ]);

    assert_eq!(timeline.entries().len(), 3);
    assert_eq!(timeline.entries()[0].item(), Some(&first));
    assert_eq!(timeline.entries()[0].turn_id(), "turn-1");
    assert_eq!(timeline.entries()[1].item(), Some(&replacement));
    assert_eq!(timeline.entries()[1].turn_id(), "turn-2");
    assert_eq!(timeline.entries()[2].item(), Some(&second));
}

#[test]
fn empty_provider_item_ids_keep_distinct_turn_positions() {
    let mut timeline = TimelineState::new("thread-1");
    let first = ThreadItem::Reasoning {
        id: String::new(),
        summary: vec!["first thought".to_string()],
        content: Vec::new(),
    };
    let second = ThreadItem::Reasoning {
        id: String::new(),
        summary: vec!["second thought".to_string()],
        content: Vec::new(),
    };

    timeline.replace_from_turns([
        ("turn-1", std::slice::from_ref(&first)),
        ("turn-2", std::slice::from_ref(&second)),
    ]);

    assert_eq!(timeline.entries().len(), 2);
    assert_eq!(timeline.entries()[0].item(), Some(&first));
    assert_eq!(timeline.entries()[0].turn_id(), "turn-1");
    assert_eq!(timeline.entries()[1].item(), Some(&second));
    assert_eq!(timeline.entries()[1].turn_id(), "turn-2");
}

#[test]
fn typed_plan_update_replaces_the_claude_todo_call_in_place() {
    let mut timeline = TimelineState::new("thread-1");
    let running = ThreadItem::CoreToolCall {
        id: "todo-1".to_string(),
        tool: "TodoWrite".to_string(),
        arguments: json!({
            "todos": [{"content": "Inspect projection", "status": "in_progress"}]
        }),
        status: CoreToolCallStatus::InProgress,
        result: None,
        error: None,
        duration_ms: None,
    };
    timeline.apply(&started("thread-1", "turn-1", running));
    timeline.apply(&ServerNotification::TurnPlanUpdated(
        TurnPlanUpdatedNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            explanation: Some("Keep the UI current.".to_string()),
            plan: vec![TurnPlanStep {
                step: "Inspect projection".to_string(),
                status: TurnPlanStepStatus::Completed,
            }],
        },
    ));
    timeline.apply(&completed(
        "thread-1",
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
    ));

    assert_eq!(timeline.entries().len(), 1);
    assert_eq!(timeline.entries()[0].id(), "todo-1");
    assert_eq!(
        timeline.entries()[0].presentation(),
        Some(&PresentationBlock::Todo(TodoPresentation {
            explanation: Some("Keep the UI current.".to_string()),
            items: vec![TodoItemPresentation {
                text: "Inspect projection".to_string(),
                status: TodoStatus::Completed,
            }],
        }))
    );
}

#[test]
fn codex_plan_notifications_update_one_stable_turn_entry() {
    let mut timeline = TimelineState::new("thread-1");
    for status in [
        TurnPlanStepStatus::InProgress,
        TurnPlanStepStatus::Completed,
    ] {
        timeline.apply(&ServerNotification::TurnPlanUpdated(
            TurnPlanUpdatedNotification {
                thread_id: "thread-1".to_string(),
                turn_id: "turn-1".to_string(),
                explanation: None,
                plan: vec![TurnPlanStep {
                    step: "Render checklist".to_string(),
                    status,
                }],
            },
        ));
    }

    assert_eq!(timeline.entries().len(), 1);
    assert_eq!(timeline.entries()[0].id(), "astral:turn-plan");
    assert_eq!(
        timeline.entries()[0].presentation(),
        Some(&PresentationBlock::Todo(TodoPresentation {
            explanation: None,
            items: vec![TodoItemPresentation {
                text: "Render checklist".to_string(),
                status: TodoStatus::Completed,
            }],
        }))
    );
}

#[test]
fn persisted_todo_call_replays_into_the_todo_presentation() {
    let rollout_items = vec![
        RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-1".to_string(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        })),
        RolloutItem::EventMsg(EventMsg::UserMessage(UserMessageEvent {
            message: "track the work".to_string(),
            ..Default::default()
        })),
        RolloutItem::TranscriptItem(TranscriptItem::FunctionCall {
            id: None,
            name: "TodoWrite".to_string(),
            namespace: None,
            arguments: json!({
                "explanation": "Keep replay visible.",
                "todos": [
                    {"content": "Rebuild history", "status": "completed"},
                    {"content": "Render checklist", "status": "in_progress"}
                ]
            })
            .to_string(),
            call_id: "todo-1".to_string(),
        }),
        RolloutItem::TranscriptItem(TranscriptItem::FunctionCallOutput {
            call_id: "todo-1".to_string(),
            output: FunctionCallOutputPayload {
                body: codex_protocol::models::FunctionCallOutputBody::Text(
                    "Todos updated".to_string(),
                ),
                success: Some(true),
            },
        }),
        RolloutItem::EventMsg(EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-1".to_string(),
            last_agent_message: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        })),
    ];
    let turns = build_turns_from_rollout_items(&rollout_items);
    let mut timeline = TimelineState::new("thread-1");
    timeline.replace_from_turns(
        turns
            .iter()
            .map(|turn| (turn.id.as_str(), turn.items.as_slice())),
    );
    let entry = timeline
        .entries()
        .iter()
        .find(|entry| entry.id() == "todo-1")
        .expect("replayed todo entry");

    assert_eq!(
        entry
            .item()
            .and_then(|item| PresentationBlock::from_item(item, entry.stream())),
        Some(PresentationBlock::Todo(TodoPresentation {
            explanation: Some("Keep replay visible.".to_string()),
            items: vec![
                TodoItemPresentation {
                    text: "Rebuild history".to_string(),
                    status: TodoStatus::Completed,
                },
                TodoItemPresentation {
                    text: "Render checklist".to_string(),
                    status: TodoStatus::InProgress,
                },
            ],
        }))
    );
}

#[test]
fn background_interactions_follow_process_id_and_output_follows_call_id() {
    let mut timeline = TimelineState::new("thread-1");
    timeline.apply(&started(
        "thread-1",
        "turn-1",
        background_command("command-1", "process-7"),
    ));

    timeline.apply(&ServerNotification::TerminalInteraction(
        TerminalInteractionNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "stale-item-id".to_string(),
            process_id: "process-7".to_string(),
            stdin: "continue\n".to_string(),
        },
    ));
    timeline.apply(&ServerNotification::CommandExecutionOutputDelta(
        CommandExecutionOutputDeltaNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "command-1".to_string(),
            delta: "done\n".to_string(),
        },
    ));

    assert_eq!(timeline.entries().len(), 1);
    assert_eq!(
        timeline.entries()[0].stream(),
        &TimelineStream::Command {
            process_id: Some("process-7".to_string()),
            output: "done\n".to_string(),
            terminal_input: vec!["continue\n".to_string()],
        }
    );
}
