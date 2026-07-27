use astral_tui_scrollback::PresentationBlock;
use pretty_assertions::assert_eq;

use crate::conversation::TranscriptBlock;
use crate::conversation::TranscriptTurn;

use super::format_duration;
use super::item_duration_ms;

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
