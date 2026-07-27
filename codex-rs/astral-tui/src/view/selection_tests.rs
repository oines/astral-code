use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::text::Text;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use std::time::Duration;
use std::time::Instant;

use super::DEFAULT_SELECTION_HIGHLIGHT_DURATION;
use super::ScrollbackSelection;
use super::ScrollbackSelectionAction;
use super::ScrollbackViewport;
use super::slice_display_columns;
use crate::view::AstralTheme;
use crate::view::transcript::TranscriptLayout;
use crate::view::transcript::TranscriptSection;

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn layout() -> TranscriptLayout {
    TranscriptLayout {
        lines: vec!["alpha".into(), "你好吗".into(), "omega".into()],
        sections: vec![
            TranscriptSection {
                item_id: "first".to_string(),
                lines: 0..2,
            },
            TranscriptSection {
                item_id: "second".to_string(),
                lines: 2..3,
            },
        ],
    }
}

fn render(selection: &mut ScrollbackSelection, viewport: ScrollbackViewport, area: Rect) -> Buffer {
    let layout = layout();
    let mut buffer = Buffer::empty(area);
    let visible = layout.lines[viewport.first_visible_line..viewport.end_visible_line].to_vec();
    Paragraph::new(Text::from(visible)).render(area, &mut buffer);
    selection.render(&layout, viewport, area, &mut buffer, AstralTheme::default());
    buffer
}

#[test]
fn drag_selection_copies_across_ascii_and_cjk_lines() {
    let area = Rect::new(2, 3, 12, 2);
    let viewport = ScrollbackViewport::from_first(3, 2, 0);
    let mut selection = ScrollbackSelection::default();
    render(&mut selection, viewport, area);

    assert_eq!(
        selection.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 3, 3)),
        ScrollbackSelectionAction::Redraw
    );
    render(&mut selection, viewport, area);
    assert_eq!(
        selection.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 4, 4)),
        ScrollbackSelectionAction::ScrollDown
    );
    render(&mut selection, viewport, area);

    assert_eq!(
        selection.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 4, 4)),
        ScrollbackSelectionAction::Copy("lpha\n你好".to_string())
    );
}

#[test]
fn persistent_selection_reprojects_after_scrolling() {
    let area = Rect::new(0, 0, 12, 2);
    let first = ScrollbackViewport::from_first(3, 2, 0);
    let mut selection = ScrollbackSelection::default();
    render(&mut selection, first, area);
    selection.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 0, 0));
    render(&mut selection, first, area);
    selection.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 2, 1));
    render(&mut selection, first, area);
    selection.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 2, 1));

    let scrolled = ScrollbackViewport::from_first(3, 2, 1);
    let buffer = render(&mut selection, scrolled, area);
    let theme = AstralTheme::default();

    assert_eq!(buffer[(0, 0)].bg, theme.text_primary);
    assert_eq!(buffer[(0, 0)].fg, theme.bg_base);
    assert_eq!(buffer[(2, 0)].bg, theme.text_primary);
    assert_eq!(buffer[(4, 0)].bg, Color::Reset);
}

#[test]
fn drag_head_stays_inside_the_anchor_range() {
    let area = Rect::new(0, 0, 12, 3);
    let viewport = ScrollbackViewport::from_first(3, 3, 0);
    let mut selection = ScrollbackSelection::default();
    render(&mut selection, viewport, area);
    selection.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 0, 0));
    render(&mut selection, viewport, area);
    selection.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 11, 2));
    render(&mut selection, viewport, area);

    assert_eq!(
        selection.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 11, 2)),
        ScrollbackSelectionAction::Copy("alpha\n你好吗".to_string())
    );
    let buffer = render(&mut selection, viewport, area);
    let theme = AstralTheme::default();
    assert_eq!(buffer[(0, 1)].bg, theme.text_primary);
    assert_eq!(buffer[(0, 2)].bg, Color::Reset);
}

#[test]
fn clicking_outside_scrollback_clears_the_persistent_selection() {
    let area = Rect::new(2, 3, 12, 2);
    let viewport = ScrollbackViewport::from_first(3, 2, 0);
    let mut selection = ScrollbackSelection::default();
    render(&mut selection, viewport, area);
    selection.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 2, 3));
    render(&mut selection, viewport, area);
    selection.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 4, 3));
    render(&mut selection, viewport, area);
    selection.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 4, 3));

    assert_eq!(
        selection.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 0, 0)),
        ScrollbackSelectionAction::Redraw
    );
    let buffer = render(&mut selection, viewport, area);
    assert_eq!(buffer[(2, 3)].bg, Color::Reset);
}

#[test]
fn copied_selection_expires_after_the_grok_flash_duration() {
    let area = Rect::new(0, 0, 12, 2);
    let viewport = ScrollbackViewport::from_first(3, 2, 0);
    let mut selection = ScrollbackSelection::default();
    render(&mut selection, viewport, area);
    selection.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 0, 0));
    render(&mut selection, viewport, area);
    selection.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 2, 0));
    render(&mut selection, viewport, area);
    selection.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 2, 0));
    selection.persistent_created_at =
        Some(Instant::now() - DEFAULT_SELECTION_HIGHLIGHT_DURATION - Duration::from_millis(1));

    assert!(selection.expire_if_due(Instant::now()));
    let buffer = render(&mut selection, viewport, area);
    assert_eq!(buffer[(0, 0)].bg, Color::Reset);
}

#[test]
fn display_column_slicing_keeps_wide_characters_whole() {
    assert_eq!(slice_display_columns("a你好z", 1..4), "你好");
    assert_eq!(slice_display_columns("a你好z", 2..3), "你");
}
