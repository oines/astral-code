use astral_tui_scrollback::DisplayMode;
use astral_tui_scrollback::PresentationBlock;
use astral_tui_scrollback::ToolKind;
use astral_tui_scrollback::ToolOrigin;
use astral_tui_scrollback::ToolPresentation;
use astral_tui_scrollback::ToolStatus;
use pretty_assertions::assert_eq;

use crate::conversation::TranscriptBlock;
use crate::conversation::TranscriptTurn;

use super::EntryDisplayState;
use super::entry_id;
use super::scan_turn;

fn block(item_id: &str, block: PresentationBlock) -> TranscriptBlock {
    TranscriptBlock {
        item_id: item_id.to_string(),
        block,
        started_at_ms: None,
        completed_at_ms: None,
    }
}

fn turn(blocks: Vec<TranscriptBlock>) -> TranscriptTurn {
    TranscriptTurn {
        id: "turn-1".to_string(),
        blocks,
        started_at_ms: None,
        completed_at_ms: None,
        duration_ms: None,
    }
}

fn tool(status: ToolStatus) -> PresentationBlock {
    tool_with_kind(ToolKind::Execute, status)
}

fn tool_with_kind(kind: ToolKind, status: ToolStatus) -> PresentationBlock {
    PresentationBlock::Tool(ToolPresentation {
        kind,
        origin: ToolOrigin::Agent,
        status,
        name: "exec".to_string(),
        title: "cargo test".to_string(),
        details: vec!["cwd /workspace".to_string()],
        output: Some("test output".to_string()),
        changes: Vec::new(),
        duration_ms: None,
    })
}

fn thinking(running: bool) -> PresentationBlock {
    PresentationBlock::Thinking {
        text: "Inspect the renderer".to_string(),
        running,
    }
}

#[test]
fn focus_navigates_only_foldable_entries_and_preserves_manual_modes() {
    let turns = [turn(vec![
        block(
            "assistant",
            PresentationBlock::Assistant {
                text: "Done.".to_string(),
            },
        ),
        block("tool", tool(ToolStatus::Success)),
        block(
            "thinking",
            PresentationBlock::Thinking {
                text: "Inspect the renderer".to_string(),
                running: false,
            },
        ),
    ])];
    let mut state = EntryDisplayState::default();
    state.observe(&turns);

    assert!(state.focus_scrollback());
    assert_eq!(
        state.selected_id(),
        Some(entry_id("turn-1", "thinking").as_str())
    );
    assert_eq!(state.move_selection(-1), Some(entry_id("turn-1", "tool")));
    assert_eq!(state.toggle_selected(), Some(entry_id("turn-1", "tool")));
    assert_eq!(
        state.mode_for("turn-1", "tool", &turns[0].blocks[1].block),
        DisplayMode::Truncated
    );

    state.observe(&turns);
    assert_eq!(
        state.mode_for("turn-1", "tool", &turns[0].blocks[1].block),
        DisplayMode::Truncated
    );
}

#[test]
fn defaults_follow_entry_lifecycle_until_the_user_pins_a_fold() {
    let mut state = EntryDisplayState::default();
    let running = turn(vec![block("thinking", thinking(true))]);
    state.observe(std::slice::from_ref(&running));
    assert_eq!(
        state.mode_for("turn-1", "thinking", &running.blocks[0].block),
        DisplayMode::Truncated
    );

    let finished = turn(vec![block("thinking", thinking(false))]);
    state.observe(std::slice::from_ref(&finished));
    assert_eq!(
        state.mode_for("turn-1", "thinking", &finished.blocks[0].block),
        DisplayMode::Collapsed
    );

    assert!(state.focus_scrollback());
    state.expand_selected();
    state.observe(std::slice::from_ref(&finished));
    assert_eq!(
        state.mode_for("turn-1", "thinking", &finished.blocks[0].block),
        DisplayMode::Expanded
    );

    state.toggle_all_thinking();
    assert_eq!(
        state.mode_for("turn-1", "thinking", &finished.blocks[0].block),
        DisplayMode::Collapsed
    );
    state.toggle_all_thinking();
    assert_eq!(
        state.mode_for("turn-1", "thinking", &finished.blocks[0].block),
        DisplayMode::Expanded
    );

    let next = turn(vec![block("thinking-next", thinking(false))]);
    state.observe(std::slice::from_ref(&next));
    assert_eq!(
        state.mode_for("turn-1", "thinking-next", &next.blocks[0].block),
        DisplayMode::Expanded
    );

    state.toggle_all();
    assert_eq!(
        state.mode_for("turn-1", "thinking-next", &next.blocks[0].block),
        DisplayMode::Collapsed
    );
}

#[test]
fn truncated_entry_expands_on_the_first_toggle() {
    let turns = [turn(vec![block("thinking", thinking(true))])];
    let mut state = EntryDisplayState::default();
    state.observe(&turns);
    assert!(state.focus_scrollback());

    state.toggle_selected();

    assert_eq!(
        state.mode_for("turn-1", "thinking", &turns[0].blocks[0].block),
        DisplayMode::Expanded
    );
}

#[test]
fn selected_entries_use_their_block_specific_fold_cycle() {
    let turns = [turn(vec![
        block("execute", tool(ToolStatus::Success)),
        block("read", tool_with_kind(ToolKind::Read, ToolStatus::Success)),
        block("thinking", thinking(true)),
    ])];
    let mut state = EntryDisplayState::default();
    state.observe(&turns);

    assert!(state.select(&entry_id("turn-1", "execute")));
    state.toggle_selected();
    assert_eq!(state.selected_mode(), Some(DisplayMode::Truncated));
    state.toggle_selected();
    assert_eq!(state.selected_mode(), Some(DisplayMode::Collapsed));

    assert!(state.select(&entry_id("turn-1", "read")));
    state.toggle_selected();
    state.observe(&turns);
    assert!(!state.selected_is_group_header());
    state.toggle_selected();
    assert_eq!(state.selected_mode(), Some(DisplayMode::Truncated));

    assert!(state.select(&entry_id("turn-1", "thinking")));
    state.toggle_selected();
    assert_eq!(state.selected_mode(), Some(DisplayMode::Expanded));
    state.toggle_selected();
    assert_eq!(state.selected_mode(), Some(DisplayMode::Truncated));
    state.expand_selected();
    state.collapse_selected();
    assert_eq!(state.selected_mode(), Some(DisplayMode::Truncated));
}

#[test]
fn verb_group_and_first_member_keep_independent_fold_state() {
    let turns = [turn(vec![
        block(
            "read-1",
            tool_with_kind(ToolKind::Read, ToolStatus::Success),
        ),
        block(
            "search-1",
            tool_with_kind(ToolKind::Search, ToolStatus::Success),
        ),
        block(
            "read-2",
            tool_with_kind(ToolKind::Read, ToolStatus::Success),
        ),
    ])];
    let mut state = EntryDisplayState::default();
    state.observe(&turns);
    let group_id = scan_turn(&turns[0], &state)[0].id.clone();
    assert!(state.focus_scrollback());
    assert_eq!(state.selected_id(), Some(group_id.as_str()));
    assert_eq!(state.selected_mode(), Some(DisplayMode::Collapsed));

    state.toggle_selected();
    state.observe(&turns);
    assert!(state.group_is_expanded(&group_id));
    assert_eq!(state.selected_id(), Some(group_id.as_str()));
    assert!(!state.selected_is_group_header());
    assert_eq!(state.selected_mode(), Some(DisplayMode::Collapsed));

    state.expand_selected();
    state.observe(&turns);
    let rekeyed_group_id = scan_turn(&turns[0], &state)[0].id.clone();
    assert_ne!(rekeyed_group_id, group_id);
    assert!(state.group_is_expanded(&rekeyed_group_id));
    assert!(scan_turn(&turns[0], &state)[0].expanded);
    assert_eq!(
        state.mode_for("turn-1", "read-1", &turns[0].blocks[0].block),
        DisplayMode::Expanded
    );

    state.collapse_selected();
    state.observe(&turns);
    assert!(state.group_is_expanded(&group_id));
    assert!(scan_turn(&turns[0], &state)[0].expanded);
    assert_eq!(state.selected_mode(), Some(DisplayMode::Collapsed));

    state.collapse_selected();
    state.observe(&turns);
    assert!(!state.group_is_expanded(&group_id));
    assert_eq!(state.selected_mode(), Some(DisplayMode::Collapsed));
}

#[test]
fn expanding_dense_truncation_clears_selection_until_navigation() {
    let turns = [turn(
        (0..13)
            .map(|index| block(&format!("command-{index}"), tool(ToolStatus::Success)))
            .collect(),
    )];
    let group_id = entry_id("turn-1", "command-0");
    let mut state = EntryDisplayState::default();
    state.observe(&turns);
    assert!(state.select(&group_id));

    assert_eq!(state.toggle_selected(), Some(group_id.clone()));
    state.observe(&turns);

    assert!(state.group_is_expanded(&group_id));
    assert_eq!(state.selected_id(), None);
    assert_eq!(state.move_selection(1), Some(group_id.clone()));
    assert_eq!(
        state.move_selection(1),
        Some(entry_id("turn-1", "command-1"))
    );
}

#[test]
fn left_from_dense_group_member_collapses_to_the_group_header() {
    let turns = [turn(
        (0..13)
            .map(|index| block(&format!("command-{index}"), tool(ToolStatus::Success)))
            .collect(),
    )];
    let group_id = entry_id("turn-1", "command-0");
    let member_id = entry_id("turn-1", "command-2");
    let mut state = EntryDisplayState::default();
    state.observe(&turns);
    state.select(&group_id);
    state.toggle_selected();
    state.observe(&turns);
    state.select(&member_id);

    assert_eq!(state.collapse_selected(), Some(group_id.clone()));
    state.observe(&turns);

    assert!(!state.group_is_expanded(&group_id));
    assert_eq!(state.selected_id(), Some(group_id.as_str()));
}
