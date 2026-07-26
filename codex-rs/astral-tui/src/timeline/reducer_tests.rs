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
use pretty_assertions::assert_eq;
use serde_json::json;

use super::ReduceOutcome;
use super::TimelineState;
use astral_tui_scrollback::TimelineStream;

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
fn replay_upserts_same_item_id_in_place() {
    let mut timeline = TimelineState::new("thread-1");
    let first = agent_message("message-1", "first");
    let replacement = agent_message("message-1", "replacement");
    let second = agent_message("message-2", "second");

    timeline.replace_from_turns([
        ("turn-1", std::slice::from_ref(&first)),
        ("turn-2", &[replacement.clone(), second.clone()]),
    ]);

    assert_eq!(timeline.entries().len(), 2);
    assert_eq!(timeline.entries()[0].item(), Some(&replacement));
    assert_eq!(timeline.entries()[0].turn_id(), "turn-1");
    assert_eq!(timeline.entries()[1].item(), Some(&second));
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
