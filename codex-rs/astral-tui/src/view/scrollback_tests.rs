use astral_tui_scrollback::LineJoiner;
use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;

use super::ScrollbackNavigation;
use super::ScrollbackPane;
use super::ScrollbackViewport;
use crate::view::AstralTheme;
use crate::view::transcript::TranscriptLayout;
use crate::view::transcript::TranscriptSection;
use crate::view::transcript::TranscriptSectionKind;
use crate::view::transcript::TranscriptSelectableLine;
use crate::view::transcript::TranscriptSelectableRange;

fn transcript_layout(sections: &[(&str, usize)]) -> TranscriptLayout {
    let mut lines = Vec::new();
    let mut ranges = Vec::new();
    let mut selectable_ranges = Vec::new();
    for (item_id, height) in sections {
        let start = lines.len();
        lines.extend((0..*height).map(|line| Line::from(format!("{item_id} line {line:02}"))));
        ranges.push(TranscriptSection {
            item_id: (*item_id).to_string(),
            lines: start..lines.len(),
            kind: TranscriptSectionKind::Entry,
            accent: None,
        });
        selectable_ranges.push(TranscriptSelectableRange {
            lines: (start..lines.len())
                .map(|line| TranscriptSelectableLine {
                    line,
                    columns: 0..u16::try_from(lines[line].width()).unwrap_or(u16::MAX),
                    joiner_to_previous: LineJoiner::HardBreak,
                })
                .collect(),
        });
    }
    TranscriptLayout {
        lines,
        sections: ranges,
        groups: Vec::new(),
        selectable_ranges,
        links: Vec::new(),
    }
}

fn buffer_text(buffer: &Buffer) -> String {
    let area = buffer.area;
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn viewport_follows_the_tail_at_zero_distance() {
    assert_eq!(
        ScrollbackViewport::measure(40, 10, 0),
        ScrollbackViewport {
            first_visible_line: 30,
            end_visible_line: 40,
            total_lines: 40,
            viewport_lines: 10,
            has_content_above: true,
            has_content_below: false,
        }
    );
}

#[test]
fn viewport_exposes_content_below_when_scrolled_up() {
    assert_eq!(
        ScrollbackViewport::measure(40, 10, 7),
        ScrollbackViewport {
            first_visible_line: 23,
            end_visible_line: 33,
            total_lines: 40,
            viewport_lines: 10,
            has_content_above: true,
            has_content_below: true,
        }
    );
}

#[test]
fn viewport_clamps_at_the_top() {
    assert_eq!(
        ScrollbackViewport::measure(8, 20, usize::MAX),
        ScrollbackViewport {
            first_visible_line: 0,
            end_visible_line: 8,
            total_lines: 8,
            viewport_lines: 20,
            has_content_above: false,
            has_content_below: false,
        }
    );
}

#[test]
fn manual_anchor_does_not_move_when_the_tail_grows() {
    let initial = transcript_layout(&[("read", 15), ("tail", 10)]);
    let mut navigation = ScrollbackNavigation::default();
    navigation.prepare(&initial, 40, 5);
    navigation.scroll_up(/*lines*/ 12);
    let before = navigation.prepare(&initial, 40, 5);
    assert_eq!(before.first_visible_line, 8);

    let grown = transcript_layout(&[("read", 15), ("tail", 20)]);
    let after = navigation.prepare(&grown, 40, 5);
    assert_eq!(after.first_visible_line, 8);
    assert_eq!(navigation.distance_from_bottom(), 22);

    let area = Rect::new(0, 0, 18, 5);
    let mut buffer = Buffer::empty(area);
    ScrollbackPane {
        lines: &grown.lines,
        viewport: after,
    }
    .render(
        Rect::new(0, 0, 17, 5),
        Rect::new(17, 0, 1, 5),
        &mut buffer,
        AstralTheme::default(),
    );
    assert_snapshot!("manual_anchor_after_stream_growth", buffer_text(&buffer));
}

#[test]
fn manual_anchor_does_not_move_when_a_new_turn_appends() {
    let initial = transcript_layout(&[("read", 15), ("tail", 10)]);
    let mut navigation = ScrollbackNavigation::default();
    navigation.prepare(&initial, 40, 5);
    navigation.scroll_up(/*lines*/ 12);
    let before = navigation.prepare(&initial, 40, 5);
    assert_eq!(before.first_visible_line, 8);

    let appended = transcript_layout(&[("read", 15), ("tail", 10), ("next-user", 3)]);
    let after = navigation.prepare(&appended, 40, 5);
    assert_eq!(after.first_visible_line, 8);
    assert_eq!(navigation.distance_from_bottom(), 15);
}

#[test]
fn manual_scroll_keeps_exact_offset_between_entries() {
    let mut layout = transcript_layout(&[("first", 5), ("second", 5)]);
    layout.lines.splice(5..5, (0..4).map(|_| Line::from("")));
    layout.sections[1].lines = 9..14;

    let mut navigation = ScrollbackNavigation::default();
    navigation.prepare(&layout, 40, 5);
    navigation.scroll_up(/*lines*/ 3);

    assert_eq!(navigation.prepare(&layout, 40, 5).first_visible_line, 6);
}

#[test]
fn item_anchor_survives_reflow_before_and_inside_the_item() {
    let initial = transcript_layout(&[("before", 10), ("anchor", 10)]);
    let mut navigation = ScrollbackNavigation::default();
    navigation.prepare(&initial, 80, 5);
    navigation.scroll_up(/*lines*/ 3);
    let before = navigation.prepare(&initial, 80, 5);
    assert_eq!(before.first_visible_line, 12);

    let reflowed = transcript_layout(&[("before", 15), ("anchor", 20)]);
    let after = navigation.prepare(&reflowed, 40, 5);
    assert_eq!(after.first_visible_line, 19);
}

#[test]
fn scrolling_past_the_bottom_reenables_follow_mode() {
    let initial = transcript_layout(&[("history", 20)]);
    let mut navigation = ScrollbackNavigation::default();
    navigation.prepare(&initial, 40, 5);
    navigation.scroll_up(/*lines*/ 5);
    navigation.scroll_down(/*lines*/ 5);
    assert_eq!(navigation.distance_from_bottom(), 0);
    assert!(!navigation.follow_mode);

    navigation.scroll_down(/*lines*/ 1);
    assert!(navigation.follow_mode);

    let grown = transcript_layout(&[("history", 25)]);
    let viewport = navigation.prepare(&grown, 40, 5);
    assert_eq!(viewport.first_visible_line, 20);
    assert!(!viewport.has_content_below);
}
