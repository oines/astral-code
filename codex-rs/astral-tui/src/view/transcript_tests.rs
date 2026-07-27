use astral_tui_scrollback::PresentationBlock;
use pretty_assertions::assert_eq;

use crate::conversation::TranscriptBlock;
use crate::conversation::TranscriptTurn;

use super::AstralTheme;
use super::TranscriptSection;
use super::format_duration;
use super::item_duration_ms;
use super::render_transcript;

#[test]
fn duration_format_matches_grok_turn_markers() {
    assert_eq!(format_duration(300), "0.3s");
    assert_eq!(format_duration(2_400), "2.4s");
    assert_eq!(format_duration(125_000), "2m5s");
}

#[test]
fn item_duration_uses_lifecycle_timestamps() {
    let block = TranscriptBlock {
        item_id: "reasoning-1".to_string(),
        block: PresentationBlock::Thinking {
            text: "inspect".to_string(),
            running: false,
        },
        started_at_ms: Some(1_000),
        completed_at_ms: Some(1_500),
    };
    assert_eq!(item_duration_ms(&block), Some(500));
}

#[test]
fn turn_projection_keeps_block_and_timing_together() {
    let turn = TranscriptTurn {
        id: "turn-1".to_string(),
        blocks: vec![TranscriptBlock {
            item_id: "agent-1".to_string(),
            block: PresentationBlock::Assistant {
                text: "done".to_string(),
            },
            started_at_ms: Some(1_000),
            completed_at_ms: Some(2_000),
        }],
        started_at_ms: Some(1_000),
        completed_at_ms: Some(3_400),
        duration_ms: Some(2_400),
    };
    assert_eq!(turn.blocks[0].item_id, "agent-1");
    assert_eq!(turn.duration_ms, Some(2_400));
}

#[test]
fn transcript_layout_assigns_stable_item_sections() {
    let turn = TranscriptTurn {
        id: "turn-1".to_string(),
        blocks: vec![
            TranscriptBlock {
                item_id: "agent-1".to_string(),
                block: PresentationBlock::Assistant {
                    text: "first".to_string(),
                },
                started_at_ms: Some(1_000),
                completed_at_ms: Some(2_000),
            },
            TranscriptBlock {
                item_id: "agent-2".to_string(),
                block: PresentationBlock::Assistant {
                    text: "second".to_string(),
                },
                started_at_ms: Some(2_000),
                completed_at_ms: Some(3_000),
            },
        ],
        started_at_ms: Some(1_000),
        completed_at_ms: Some(3_400),
        duration_ms: Some(2_400),
    };

    let layout = render_transcript(&[turn], 80, AstralTheme::default());

    assert_eq!(
        layout.sections,
        vec![
            TranscriptSection {
                item_id: "turn-1\0agent-1".to_string(),
                lines: 0..1,
            },
            TranscriptSection {
                item_id: "turn-1\0agent-2".to_string(),
                lines: 1..4,
            },
        ]
    );
    assert_eq!(layout.lines.len(), 4);
}

#[test]
fn transcript_sections_scope_empty_item_ids_to_their_turn() {
    let turns = ["first", "second"].map(|label| TranscriptTurn {
        id: format!("turn-{label}"),
        blocks: vec![TranscriptBlock {
            item_id: String::new(),
            block: PresentationBlock::Thinking {
                text: format!("{label} thought"),
                running: false,
            },
            started_at_ms: None,
            completed_at_ms: None,
        }],
        started_at_ms: None,
        completed_at_ms: None,
        duration_ms: None,
    });

    let layout = render_transcript(&turns, 80, AstralTheme::default());

    assert_eq!(
        layout
            .sections
            .iter()
            .map(|section| section.item_id.as_str())
            .collect::<Vec<_>>(),
        vec!["turn-first\0", "turn-second\0"]
    );
}
