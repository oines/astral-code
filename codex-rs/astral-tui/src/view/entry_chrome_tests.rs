use astral_tui_scrollback::LineJoiner;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

use super::render_entry_chrome;
use crate::view::AstralTheme;
use crate::view::EntryChromeState;
use crate::view::ScrollbackViewport;
use crate::view::transcript::TranscriptAccent;
use crate::view::transcript::TranscriptGroup;
use crate::view::transcript::TranscriptLayout;
use crate::view::transcript::TranscriptSection;
use crate::view::transcript::TranscriptSectionKind;
use crate::view::transcript::TranscriptSelectableLine;
use crate::view::transcript::TranscriptSelectableRange;

#[test]
fn accent_and_selection_box_use_separate_gutter_columns() {
    let layout = TranscriptLayout {
        lines: vec!["one".into(), "two".into(), "three".into()],
        sections: vec![TranscriptSection {
            item_id: "entry".to_string(),
            lines: 0..3,
            kind: TranscriptSectionKind::Entry,
            accent: Some(TranscriptAccent::Full(Color::Cyan)),
        }],
        groups: Vec::new(),
        selectable_ranges: vec![TranscriptSelectableRange {
            lines: (0..3)
                .map(|line| TranscriptSelectableLine {
                    line,
                    columns: 0..5,
                    joiner_to_previous: LineJoiner::HardBreak,
                })
                .collect(),
        }],
    };
    let viewport = ScrollbackViewport::from_first(3, 3, 0);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 14, 6));

    render_entry_chrome(
        &layout,
        viewport,
        Rect::new(2, 1, 10, 3),
        EntryChromeState {
            selected_id: Some("entry"),
            ..EntryChromeState::default()
        },
        &mut buffer,
        AstralTheme::default(),
    );

    assert_eq!(buffer[(1, 1)].symbol(), "┃");
    assert_eq!(buffer[(1, 2)].symbol(), "┃");
    assert_eq!(buffer[(0, 0)].symbol(), "┌");
    assert_eq!(buffer[(13, 0)].symbol(), "┐");
    assert_eq!(buffer[(0, 4)].symbol(), "└");
    assert_eq!(buffer[(13, 4)].symbol(), "┘");
}

#[test]
fn expanded_verb_group_selection_wraps_the_whole_group() {
    let layout = TranscriptLayout {
        lines: vec![
            "group".into(),
            "member one".into(),
            "member two".into(),
            "after".into(),
        ],
        sections: vec![
            TranscriptSection {
                item_id: "one".to_string(),
                lines: 0..1,
                kind: TranscriptSectionKind::GroupHeader,
                accent: Some(TranscriptAccent::Collapsed(Color::DarkGray)),
            },
            TranscriptSection {
                item_id: "one".to_string(),
                lines: 1..2,
                kind: TranscriptSectionKind::Entry,
                accent: None,
            },
            TranscriptSection {
                item_id: "two".to_string(),
                lines: 2..3,
                kind: TranscriptSectionKind::Entry,
                accent: None,
            },
            TranscriptSection {
                item_id: "after".to_string(),
                lines: 3..4,
                kind: TranscriptSectionKind::Entry,
                accent: None,
            },
        ],
        groups: vec![TranscriptGroup {
            lines: 0..3,
            member_ids: vec!["one".to_string(), "two".to_string()],
            expanded: true,
        }],
        selectable_ranges: Vec::new(),
    };
    let viewport = ScrollbackViewport::from_first(4, 4, 0);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 14, 7));

    render_entry_chrome(
        &layout,
        viewport,
        Rect::new(2, 1, 10, 4),
        EntryChromeState {
            selected_id: Some("two"),
            ..EntryChromeState::default()
        },
        &mut buffer,
        AstralTheme::default(),
    );

    assert_eq!(buffer[(0, 0)].symbol(), "┌");
    assert_eq!(buffer[(0, 1)].symbol(), "│");
    assert_eq!(buffer[(0, 3)].symbol(), "│");
    assert_eq!(buffer[(0, 4)].symbol(), "└");
}
