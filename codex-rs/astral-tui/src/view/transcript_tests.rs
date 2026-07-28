use astral_tui_scrollback::LineJoiner;
use astral_tui_scrollback::PresentationBlock;
use pretty_assertions::assert_eq;
use ratatui::text::Line;

use crate::CommittedBlock;
use crate::conversation::TranscriptBlock;
use crate::conversation::TranscriptTurn;

use super::AstralTheme;
use super::TranscriptSection;
use super::format_duration;
use super::item_duration_ms;
use super::render_committed_block;
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
                lines: 1..3,
            },
        ]
    );
    assert_eq!(layout.lines.len(), 3);
}

#[test]
fn transcript_omits_timestamp_chrome_and_tracks_soft_wraps() {
    let turn = TranscriptTurn {
        id: "turn-1".to_string(),
        blocks: vec![TranscriptBlock {
            item_id: "agent-1".to_string(),
            block: PresentationBlock::Assistant {
                text: "alpha beta gamma".to_string(),
            },
            started_at_ms: None,
            completed_at_ms: Some(1_000),
        }],
        started_at_ms: None,
        completed_at_ms: None,
        duration_ms: None,
    };

    let layout = render_transcript(&[turn], 10, AstralTheme::default());
    let selectable = &layout.selectable_ranges[0].lines;

    assert_eq!(
        layout.lines.iter().map(Line::width).collect::<Vec<_>>(),
        vec![10, 5]
    );
    assert_eq!(selectable[0].columns, 0..10);
    assert_eq!(selectable[0].joiner_to_previous, LineJoiner::HardBreak);
    assert_eq!(selectable[1].columns, 0..5);
    assert_eq!(selectable[1].joiner_to_previous, LineJoiner::Space);
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

#[test]
fn full_and_committed_paths_render_identical_entry_boundaries() {
    let blocks = vec![
        TranscriptBlock {
            item_id: "user".to_string(),
            block: PresentationBlock::User {
                text: "inspect this repo".to_string(),
                attachments: Vec::new(),
            },
            started_at_ms: Some(1_000),
            completed_at_ms: Some(1_100),
        },
        TranscriptBlock {
            item_id: "reasoning".to_string(),
            block: PresentationBlock::Thinking {
                text: "trace the renderer".to_string(),
                running: false,
            },
            started_at_ms: Some(1_100),
            completed_at_ms: Some(1_600),
        },
        TranscriptBlock {
            item_id: "assistant".to_string(),
            block: PresentationBlock::Assistant {
                text: "Done.".to_string(),
            },
            started_at_ms: Some(1_600),
            completed_at_ms: Some(3_400),
        },
    ];
    let turn = TranscriptTurn {
        id: "turn-1".to_string(),
        blocks: blocks.clone(),
        started_at_ms: Some(1_000),
        completed_at_ms: Some(3_400),
        duration_ms: Some(2_400),
    };
    let full = render_transcript(&[turn], 80, AstralTheme::default()).lines;
    let committed = blocks
        .into_iter()
        .enumerate()
        .flat_map(|(index, block)| {
            render_committed_block(
                &CommittedBlock {
                    item_id: block.item_id,
                    turn_id: "turn-1".to_string(),
                    block: block.block,
                    started_at_ms: block.started_at_ms,
                    completed_at_ms: block.completed_at_ms,
                    turn_started_at_ms: Some(1_000),
                    turn_completed_at_ms: Some(3_400),
                    turn_duration_ms: Some(2_400),
                    ends_turn: index == 2,
                },
                80,
                AstralTheme::default(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(committed, full);
}
