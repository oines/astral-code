use astral_tui_scrollback::DisplayMode;
use astral_tui_scrollback::PresentationBlock;
use astral_tui_scrollback::ToolKind;
use astral_tui_scrollback::ToolPresentation;
use astral_tui_scrollback::ToolStatus;
use pretty_assertions::assert_eq;

use crate::conversation::TranscriptBlock;
use crate::conversation::TranscriptTurn;

use super::EntryDisplayState;
use super::entry_id;

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
    PresentationBlock::Tool(ToolPresentation {
        kind: ToolKind::Execute,
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
        DisplayMode::Expanded
    );

    state.observe(&turns);
    assert_eq!(
        state.mode_for("turn-1", "tool", &turns[0].blocks[1].block),
        DisplayMode::Expanded
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
