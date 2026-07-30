use astral_tui_scrollback::PresentationBlock;
use astral_tui_scrollback::ToolKind;
use astral_tui_scrollback::ToolOrigin;
use astral_tui_scrollback::ToolPresentation;
use astral_tui_scrollback::ToolStatus;
use pretty_assertions::assert_eq;

use super::EntryGroupKind;
use super::scan_turn;
use crate::conversation::TranscriptBlock;
use crate::conversation::TranscriptTurn;
use crate::view::EntryDisplayState;

fn tool(item_id: &str, kind: ToolKind) -> TranscriptBlock {
    TranscriptBlock {
        item_id: item_id.to_string(),
        block: PresentationBlock::Tool(ToolPresentation {
            kind,
            origin: ToolOrigin::Agent,
            status: ToolStatus::Success,
            name: item_id.to_string(),
            title: item_id.to_string(),
            details: Vec::new(),
            output: Some("detail".to_string()),
            changes: Vec::new(),
            duration_ms: None,
        }),
        started_at_ms: None,
        completed_at_ms: None,
    }
}

fn thought(item_id: &str) -> TranscriptBlock {
    TranscriptBlock {
        item_id: item_id.to_string(),
        block: PresentationBlock::Thinking {
            text: "reasoning".to_string(),
            running: false,
        },
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

#[test]
fn verb_run_claims_collapsed_thoughts_and_non_destructive_tools() {
    let turn = turn(vec![
        thought("thought"),
        tool("read-a", ToolKind::Read),
        tool("search", ToolKind::Search),
        tool("read-b", ToolKind::Read),
    ]);
    let display = EntryDisplayState::default();

    let spans = scan_turn(&turn, &display);

    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].claimed, vec![0, 1, 2, 3]);
    assert_eq!(spans[0].label, "Read 2 files, Searched 1 pattern");
}

#[test]
fn command_and_edit_break_eager_verb_runs() {
    let turn = turn(vec![
        tool("read-a", ToolKind::Read),
        tool("command", ToolKind::Execute),
        tool("read-b", ToolKind::Read),
        tool("edit", ToolKind::Edit),
    ]);
    let display = EntryDisplayState::default();

    let spans = scan_turn(&turn, &display);

    assert_eq!(
        spans
            .iter()
            .map(|span| (span.range.clone(), span.label.as_str()))
            .collect::<Vec<_>>(),
        vec![(0..1, "Read 1 file"), (2..3, "Read 1 file")]
    );
}

#[test]
fn assistant_text_separates_dense_runs() {
    let mut blocks = (0..6)
        .map(|index| tool(&format!("before-{index}"), ToolKind::Execute))
        .collect::<Vec<_>>();
    blocks.push(TranscriptBlock {
        item_id: "assistant".to_string(),
        block: PresentationBlock::Assistant {
            text: "boundary".to_string(),
        },
        started_at_ms: None,
        completed_at_ms: None,
    });
    blocks.extend((0..6).map(|index| tool(&format!("after-{index}"), ToolKind::Execute)));
    let turn = turn(blocks);
    let display = EntryDisplayState::default();

    assert!(scan_turn(&turn, &display).is_empty());
}

#[test]
fn long_dense_run_keeps_the_newest_ten_entries_visible() {
    let blocks = (0..13)
        .map(|index| tool(&format!("command-{index}"), ToolKind::Execute))
        .collect();
    let turn = turn(blocks);
    let display = EntryDisplayState::default();

    let spans = scan_turn(&turn, &display);

    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].kind, EntryGroupKind::Truncation);
    assert_eq!(spans[0].range, 0..13);
    assert_eq!(spans[0].claimed, (0..13).collect::<Vec<_>>());
    assert!(spans[0].hides(0));
    assert!(spans[0].hides(1));
    assert!(spans[0].hides(2));
    assert!(!spans[0].hides(3));
    assert_eq!(spans[0].label, "Ran 3 commands");
}

#[test]
fn dense_run_at_the_grok_fold_threshold_stays_flat() {
    let blocks = (0..11)
        .map(|index| tool(&format!("command-{index}"), ToolKind::Execute))
        .collect();
    let turn = turn(blocks);

    assert!(scan_turn(&turn, &EntryDisplayState::default()).is_empty());
}

#[test]
fn eager_verb_run_claims_before_dense_truncation() {
    let mut blocks = vec![
        tool("read-a", ToolKind::Read),
        tool("read-b", ToolKind::Read),
    ];
    blocks.extend((0..12).map(|index| tool(&format!("command-{index}"), ToolKind::Execute)));
    let turn = turn(blocks);

    let spans = scan_turn(&turn, &EntryDisplayState::default());

    assert_eq!(
        spans
            .iter()
            .map(|span| (span.kind, span.range.clone(), span.label.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (EntryGroupKind::VerbRun, 0..2, "Read 2 files"),
            (EntryGroupKind::Truncation, 2..14, "Ran 2 commands"),
        ]
    );
}

#[test]
fn expanded_truncation_keeps_a_standalone_header() {
    let blocks = (0..13)
        .map(|index| tool(&format!("command-{index}"), ToolKind::Execute))
        .collect();
    let turn = turn(blocks);
    let mut display = EntryDisplayState::default();
    display.observe(std::slice::from_ref(&turn));
    display.toggle_group("turn-1\0command-0");

    let spans = scan_turn(&turn, &display);

    assert_eq!(spans.len(), 1);
    assert!(spans[0].expanded);
    assert!(spans[0].header_owns_selection());
    assert!(spans[0].hides(0));
    assert!(!(1..13).any(|index| spans[0].hides(index)));
    assert_eq!(spans[0].label, "Ran 13 commands");
}
