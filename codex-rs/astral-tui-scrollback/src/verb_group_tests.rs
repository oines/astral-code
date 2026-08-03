use std::collections::HashMap;

use codex_app_server_protocol::CoreToolCallStatus;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;
use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

use super::VerbGroupDisplayState;
use super::scan_verb_groups;
use crate::DisplayMode;
use crate::EntryBlock;
use crate::EntryDisplayState;
use crate::EntryRenderOptions;
use crate::Transcript;
use crate::TranscriptEntry;
use crate::TranscriptEntryId;
use crate::render_verb_group_header;

#[test]
fn groups_exact_lookup_entries_without_flattening_source_order() {
    let transcript = transcript(vec![
        reasoning("thought"),
        core_tool(
            "skill",
            "Read",
            json!({"file_path": "/x/skills/deploy/SKILL.md"}),
            CoreToolCallStatus::Completed,
        ),
        core_tool(
            "read",
            "Read",
            json!({"file_path": "src/lib.rs"}),
            CoreToolCallStatus::Completed,
        ),
        core_tool(
            "grep",
            "Grep",
            json!({"pattern": "needle"}),
            CoreToolCallStatus::InProgress,
        ),
        agent("boundary"),
        core_tool(
            "glob",
            "Glob",
            json!({"pattern": "**/*.rs"}),
            CoreToolCallStatus::Completed,
        ),
    ]);
    let turn = &transcript.turns()[0];

    let groups = scan_verb_groups(turn, default_display_state);

    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].range(), 0..4);
    assert_eq!(groups[0].claimed(), &[0, 1, 2, 3]);
    assert_eq!(groups[0].members(), 3);
    assert_eq!(
        groups[0].label(),
        "Reading 1 skill, Reading 1 file, Searching 1 pattern"
    );
    assert!(groups[0].running());
    assert_eq!(groups[1].range(), 5..6);
    assert_eq!(groups[1].label(), "Searched 1 pattern");

    let mut state = VerbGroupDisplayState::default();
    assert!(state.hides(&groups[0], 0));
    assert!(state.hides(&groups[0], 3));
    assert_eq!(state.toggle(&groups[0]), DisplayMode::Expanded);
    assert!(!state.hides(&groups[0], 0));
    assert_eq!(state.toggle(&groups[0]), DisplayMode::Collapsed);

    let header = render_verb_group_header(&groups[0], EntryRenderOptions::new(/*width*/ 40));
    let text = header
        .lines()
        .iter()
        .map(|line| line.line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert_snapshot!(text, @r###"
    ◇ Reading 1 skill, Reading 1 file,
      Searching 1 pattern
    "###);
}

#[test]
fn opened_member_stays_visible_inside_the_same_run() {
    let transcript = transcript(vec![
        core_tool(
            "read-a",
            "Read",
            json!({"file_path": "a.rs"}),
            CoreToolCallStatus::Completed,
        ),
        core_tool(
            "grep",
            "Grep",
            json!({"pattern": "needle"}),
            CoreToolCallStatus::Completed,
        ),
        core_tool(
            "read-b",
            "Read",
            json!({"file_path": "b.rs"}),
            CoreToolCallStatus::Completed,
        ),
    ]);
    let turn = &transcript.turns()[0];
    let mut states = turn
        .entries()
        .iter()
        .map(|entry| {
            (
                entry.id(),
                default_display_state(entry).expect("lookup display state"),
            )
        })
        .collect::<HashMap<TranscriptEntryId, EntryDisplayState>>();
    let search = &turn.entries()[1];
    assert!(
        states
            .get_mut(&search.id())
            .expect("search state")
            .expand(&EntryBlock::from_entry(search))
    );

    let groups = scan_verb_groups(turn, |entry| states.get(&entry.id()).copied());

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].range(), 0..3);
    assert_eq!(groups[0].claimed(), &[0, 2]);
    assert_eq!(groups[0].label(), "Read 2 files");
    let state = VerbGroupDisplayState::default();
    assert!(state.hides(&groups[0], 0));
    assert!(!state.hides(&groups[0], 1));
    assert!(state.hides(&groups[0], 2));
}

#[test]
fn expanded_group_rekeys_when_its_anchor_member_opens() {
    let transcript = transcript(vec![
        core_tool(
            "grep-a",
            "Grep",
            json!({"pattern": "first"}),
            CoreToolCallStatus::Completed,
        ),
        core_tool(
            "read",
            "Read",
            json!({"file_path": "a.rs"}),
            CoreToolCallStatus::Completed,
        ),
        core_tool(
            "grep-b",
            "Grep",
            json!({"pattern": "last"}),
            CoreToolCallStatus::Completed,
        ),
    ]);
    let turn = &transcript.turns()[0];
    let mut states = turn
        .entries()
        .iter()
        .map(|entry| {
            (
                entry.id(),
                default_display_state(entry).expect("lookup display state"),
            )
        })
        .collect::<HashMap<TranscriptEntryId, EntryDisplayState>>();
    let before = scan_verb_groups(turn, |entry| states.get(&entry.id()).copied());
    let mut groups = VerbGroupDisplayState::default();
    assert!(groups.expand(&before[0]));

    let first = &turn.entries()[0];
    assert!(
        states
            .get_mut(&first.id())
            .expect("search state")
            .expand(&EntryBlock::from_entry(first))
    );
    let after = scan_verb_groups(turn, |entry| states.get(&entry.id()).copied());

    assert_ne!(before[0].anchor(), after[0].anchor());
    assert!(groups.reconcile(&before, &after));
    assert_eq!(groups.mode(&after[0]), DisplayMode::Expanded);
}

fn default_display_state(entry: &TranscriptEntry) -> Option<EntryDisplayState> {
    EntryDisplayState::for_block(&EntryBlock::from_entry(entry))
}

fn transcript(items: Vec<ThreadItem>) -> Transcript {
    let mut transcript = Transcript::new("thread-1");
    let outcome = transcript.apply(&ServerNotification::TurnStarted(TurnStartedNotification {
        thread_id: "thread-1".to_string(),
        turn: Turn {
            id: "turn-1".to_string(),
            items,
            items_view: TurnItemsView::Full,
            status: TurnStatus::InProgress,
            error: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
        },
    }));
    assert_eq!(outcome, crate::ApplyOutcome::Applied);
    transcript
}

fn reasoning(id: &str) -> ThreadItem {
    ThreadItem::Reasoning {
        id: id.to_string(),
        summary: vec!["inspect".to_string()],
        content: Vec::new(),
    }
}

fn agent(id: &str) -> ThreadItem {
    ThreadItem::AgentMessage {
        id: id.to_string(),
        text: "done".to_string(),
        phase: None,
        memory_citation: None,
    }
}

fn core_tool(id: &str, tool: &str, arguments: Value, status: CoreToolCallStatus) -> ThreadItem {
    ThreadItem::CoreToolCall {
        id: id.to_string(),
        tool: tool.to_string(),
        arguments,
        status,
        result: None,
        error: None,
        duration_ms: None,
    }
}
