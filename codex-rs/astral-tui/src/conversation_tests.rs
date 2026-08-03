use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::WebSearchAction;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

use super::ConversationState;
use super::EntryDisplayAction;
use super::VerbGroupDisplayAction;
use astral_tui_scrollback::DisplayMode;

#[test]
fn snapshot_refresh_preserves_entry_choices_and_rekeys_expanded_verb_group() {
    let mut state = ConversationState::from_thread(&thread(
        "thread-1",
        vec![
            turn(
                "turn-1",
                vec![
                    web_search("search-1", "rust"),
                    web_search("search-2", "ratatui"),
                    assistant("assistant", "old"),
                ],
            ),
            turn(
                "removed-turn",
                vec![
                    web_search("removed-1", "old one"),
                    web_search("removed-2", "old two"),
                ],
            ),
        ],
    ));
    let transcript_turn = &state.transcript().turns()[0];
    let search_1 = transcript_turn.entries()[0].id();
    let assistant_id = transcript_turn.entries()[2].id();
    let removed_entry_id = state.transcript().turns()[1].entries()[0].id();
    let group = state.verb_groups("turn-1")[0].clone();
    assert_eq!(group.anchor(), search_1);
    assert_eq!(
        state.apply_verb_group_display_action(
            "turn-1",
            group.anchor(),
            VerbGroupDisplayAction::Expand,
        ),
        Some(DisplayMode::Expanded)
    );
    assert!(state.apply_entry_display_action(assistant_id, EntryDisplayAction::ToggleRaw));

    state.reset_from_thread(&thread(
        "thread-1",
        vec![turn(
            "turn-1",
            vec![
                web_search("search-0", "terminal"),
                web_search("search-1", "rust"),
                web_search("search-2", "ratatui"),
                assistant("assistant", "new"),
            ],
        )],
    ));

    let transcript_turn = &state.transcript().turns()[0];
    assert_eq!(
        transcript_turn
            .entries()
            .iter()
            .map(|entry| entry.item().id())
            .collect::<Vec<_>>(),
        vec!["search-0", "search-1", "search-2", "assistant"]
    );
    assert_eq!(transcript_turn.entries()[1].id(), search_1);
    assert_eq!(transcript_turn.entries()[3].id(), assistant_id);
    assert!(
        state
            .entry_display_state(assistant_id)
            .expect("assistant display state")
            .raw()
    );
    assert_eq!(state.entry_display_state(removed_entry_id), None);
    assert!(state.verb_groups("removed-turn").is_empty());
    let group = &state.verb_groups("turn-1")[0];
    assert_ne!(group.anchor(), search_1);
    assert_eq!(
        state.verb_group_mode("turn-1", group),
        Some(DisplayMode::Expanded)
    );

    state.reset_from_thread(&thread(
        "thread-1",
        vec![turn(
            "turn-1",
            vec![
                web_search("replacement-1", "unrelated one"),
                web_search("replacement-2", "unrelated two"),
                assistant("replacement-assistant", "replacement"),
            ],
        )],
    ));

    let replacement = &state.verb_groups("turn-1")[0];
    assert_eq!(
        state.verb_group_mode("turn-1", replacement),
        Some(DisplayMode::Collapsed)
    );
    assert_eq!(state.entry_display_state(assistant_id), None);
}

#[test]
fn thread_switch_drops_presentation_choices_even_when_protocol_ids_repeat() {
    let initial = thread(
        "thread-1",
        vec![turn(
            "turn-1",
            vec![
                web_search("search-1", "rust"),
                web_search("search-2", "ratatui"),
                assistant("assistant", "old"),
            ],
        )],
    );
    let mut state = ConversationState::from_thread(&initial);
    let old_search = state.transcript().turns()[0].entries()[0].id();
    let old_assistant = state.transcript().turns()[0].entries()[2].id();
    let old_group = state.verb_groups("turn-1")[0].clone();
    assert_eq!(
        state.apply_verb_group_display_action(
            "turn-1",
            old_group.anchor(),
            VerbGroupDisplayAction::Expand,
        ),
        Some(DisplayMode::Expanded)
    );
    assert!(state.apply_entry_display_action(old_assistant, EntryDisplayAction::ToggleRaw));

    state.reset_from_thread(&thread(
        "thread-2",
        vec![turn(
            "turn-1",
            vec![
                web_search("search-1", "rust"),
                web_search("search-2", "ratatui"),
                assistant("assistant", "new"),
            ],
        )],
    ));

    let new_turn = &state.transcript().turns()[0];
    let new_search = new_turn.entries()[0].id();
    let new_assistant = new_turn.entries()[2].id();
    assert_ne!(new_search, old_search);
    assert_ne!(new_assistant, old_assistant);
    assert!(
        !state
            .entry_display_state(new_assistant)
            .expect("assistant display state")
            .raw()
    );
    let new_group = &state.verb_groups("turn-1")[0];
    assert_eq!(
        state.verb_group_mode("turn-1", new_group),
        Some(DisplayMode::Collapsed)
    );
}

fn thread(thread_id: &str, turns: Vec<Turn>) -> Thread {
    Thread {
        id: thread_id.to_string(),
        session_id: thread_id.to_string(),
        forked_from_id: None,
        parent_thread_id: None,
        preview: String::new(),
        ephemeral: false,
        model_provider: "astral".to_string(),
        created_at: 1,
        updated_at: 1,
        status: ThreadStatus::Idle,
        path: None,
        cwd: AbsolutePathBuf::current_dir().expect("current directory should be absolute"),
        cli_version: "0.0.0".to_string(),
        source: SessionSource::Exec,
        thread_source: None,
        agent_nickname: None,
        agent_role: None,
        git_info: None,
        name: None,
        turns,
    }
}

fn turn(turn_id: &str, items: Vec<ThreadItem>) -> Turn {
    Turn {
        id: turn_id.to_string(),
        items,
        items_view: TurnItemsView::Full,
        status: TurnStatus::Completed,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }
}

fn web_search(id: &str, query: &str) -> ThreadItem {
    ThreadItem::WebSearch {
        id: id.to_string(),
        query: query.to_string(),
        action: Some(WebSearchAction::Search {
            query: Some(query.to_string()),
            queries: None,
        }),
    }
}

fn assistant(id: &str, text: &str) -> ThreadItem {
    ThreadItem::AgentMessage {
        id: id.to_string(),
        text: text.to_string(),
        phase: None,
        memory_citation: None,
    }
}
