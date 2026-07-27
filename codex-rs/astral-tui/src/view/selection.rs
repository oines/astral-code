// Derived from Grok Build's scrollback text-selection behavior at commit
// 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Modified for Astral's line-based transcript viewport.

use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::text::Line;
use std::cmp::Ordering;
use std::ops::Range;

use super::AstralTheme;
use super::ScrollbackViewport;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SelectionPoint {
    line: usize,
    column: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SelectionRange {
    anchor: SelectionPoint,
    head: SelectionPoint,
}

impl SelectionRange {
    fn normalized(self) -> (SelectionPoint, SelectionPoint) {
        if compare_points(self.anchor, self.head).is_le() {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SelectionFrame {
    area: Rect,
    viewport: ScrollbackViewport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScrollbackSelectionAction {
    Ignored,
    Redraw,
    ScrollUp,
    ScrollDown,
    Copy(String),
}

/// Mouse-driven text selection owned by Astral's scrollback buffer.
///
/// Keeping the selection in transcript coordinates lets the overlay survive
/// manual scrolling without falling back to the terminal's native selection.
#[derive(Debug, Default)]
pub(crate) struct ScrollbackSelection {
    frame: Option<SelectionFrame>,
    pending: Option<SelectionPoint>,
    active: Option<SelectionRange>,
    persistent: Option<SelectionRange>,
    lines: Vec<String>,
    render_width: u16,
}

impl ScrollbackSelection {
    pub(crate) fn handle_mouse(&mut self, mouse: MouseEvent) -> ScrollbackSelectionAction {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(point) = self.hit_test(mouse.column, mouse.row) else {
                    return if self.clear() {
                        ScrollbackSelectionAction::Redraw
                    } else {
                        ScrollbackSelectionAction::Ignored
                    };
                };
                self.pending = Some(point);
                self.active = None;
                self.persistent = None;
                ScrollbackSelectionAction::Redraw
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some(anchor) = self
                    .pending
                    .or_else(|| self.active.map(|range| range.anchor))
                else {
                    return ScrollbackSelectionAction::Ignored;
                };
                let Some(head) = self.hit_test_nearest(mouse.column, mouse.row) else {
                    return ScrollbackSelectionAction::Ignored;
                };
                if head != anchor || self.active.is_some() {
                    self.active = Some(SelectionRange { anchor, head });
                }
                let Some(frame) = self.frame else {
                    return ScrollbackSelectionAction::Redraw;
                };
                if mouse.row <= frame.area.y {
                    ScrollbackSelectionAction::ScrollUp
                } else if mouse.row >= frame.area.bottom().saturating_sub(1) {
                    ScrollbackSelectionAction::ScrollDown
                } else {
                    ScrollbackSelectionAction::Redraw
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let Some(mut range) = self.active.take() else {
                    self.pending = None;
                    return ScrollbackSelectionAction::Redraw;
                };
                if let Some(head) = self.hit_test_nearest(mouse.column, mouse.row) {
                    range.head = head;
                }
                self.pending = None;
                self.persistent = Some(range);
                self.copy_text(range)
                    .filter(|text| !text.is_empty())
                    .map(ScrollbackSelectionAction::Copy)
                    .unwrap_or(ScrollbackSelectionAction::Redraw)
            }
            _ => ScrollbackSelectionAction::Ignored,
        }
    }

    pub(crate) fn render(
        &mut self,
        lines: &[Line<'static>],
        viewport: ScrollbackViewport,
        area: Rect,
        buffer: &mut Buffer,
        theme: AstralTheme,
    ) {
        if self.render_width != 0 && self.render_width != area.width && self.clear() {
            self.lines.clear();
        }
        self.render_width = area.width;
        self.frame = Some(SelectionFrame { area, viewport });
        if self.is_tracking() {
            self.lines = lines.iter().map(Line::to_string).collect();
        } else {
            self.lines.clear();
        }
        if let Some(range) = self.active.or(self.persistent) {
            render_selection_overlay(range, &self.lines, viewport, area, buffer, theme);
        }
    }

    pub(crate) fn clear(&mut self) -> bool {
        let changed = self.pending.take().is_some()
            || self.active.take().is_some()
            || self.persistent.take().is_some();
        if changed {
            self.lines.clear();
        }
        changed
    }

    fn is_tracking(&self) -> bool {
        self.pending.is_some() || self.active.is_some() || self.persistent.is_some()
    }

    fn hit_test(&self, column: u16, row: u16) -> Option<SelectionPoint> {
        let frame = self.frame?;
        if !frame.area.contains((column, row).into()) {
            return None;
        }
        Some(point_in_frame(frame, column, row))
    }

    fn hit_test_nearest(&self, column: u16, row: u16) -> Option<SelectionPoint> {
        let frame = self.frame?;
        if frame.area.is_empty() {
            return None;
        }
        let column = column.clamp(frame.area.x, frame.area.right().saturating_sub(1));
        let row = row.clamp(frame.area.y, frame.area.bottom().saturating_sub(1));
        Some(point_in_frame(frame, column, row))
    }

    fn copy_text(&self, range: SelectionRange) -> Option<String> {
        let (start, end) = range.normalized();
        let mut selected = Vec::new();
        for line_index in start.line..=end.line {
            let line = self.lines.get(line_index)?.trim_end();
            let width = line_width(line);
            let columns = if start.line == end.line {
                start.column..end.column.saturating_add(1)
            } else if line_index == start.line {
                start.column..u16::try_from(width).unwrap_or(u16::MAX)
            } else if line_index == end.line {
                0..end.column.saturating_add(1)
            } else {
                0..u16::try_from(width).unwrap_or(u16::MAX)
            };
            selected.push(slice_display_columns(line, columns));
        }
        Some(selected.join("\n").trim_end_matches('\n').to_string())
    }
}

fn compare_points(left: SelectionPoint, right: SelectionPoint) -> Ordering {
    (left.line, left.column).cmp(&(right.line, right.column))
}

fn point_in_frame(frame: SelectionFrame, column: u16, row: u16) -> SelectionPoint {
    let visible_row = usize::from(row.saturating_sub(frame.area.y));
    SelectionPoint {
        line: frame
            .viewport
            .first_visible_line
            .saturating_add(visible_row)
            .min(frame.viewport.total_lines.saturating_sub(1)),
        column: column.saturating_sub(frame.area.x),
    }
}

fn render_selection_overlay(
    range: SelectionRange,
    lines: &[String],
    viewport: ScrollbackViewport,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    let (start, end) = range.normalized();
    for line_index in start.line..=end.line {
        if !(viewport.first_visible_line..viewport.end_visible_line).contains(&line_index) {
            continue;
        }
        let Some(line) = lines.get(line_index) else {
            continue;
        };
        let width = u16::try_from(line_width(line.trim_end())).unwrap_or(u16::MAX);
        let columns = expand_display_columns(
            line.trim_end(),
            selected_columns(start, end, line_index, width),
        );
        let y = area.y
            + u16::try_from(line_index.saturating_sub(viewport.first_visible_line))
                .unwrap_or(u16::MAX);
        for column in columns.start.min(area.width)..columns.end.min(area.width) {
            if let Some(cell) = buffer.cell_mut((area.x + column, y)) {
                apply_selection_highlight(theme, cell);
            }
        }
    }
}

fn selected_columns(
    start: SelectionPoint,
    end: SelectionPoint,
    line_index: usize,
    width: u16,
) -> Range<u16> {
    if start.line == end.line {
        start.column.min(width)..end.column.saturating_add(1).min(width)
    } else if line_index == start.line {
        start.column.min(width)..width
    } else if line_index == end.line {
        0..end.column.saturating_add(1).min(width)
    } else {
        0..width
    }
}

fn apply_selection_highlight(theme: AstralTheme, cell: &mut ratatui::buffer::Cell) {
    if theme.text_primary == Color::Reset || theme.bg_base == Color::Reset {
        cell.modifier.insert(Modifier::REVERSED);
        return;
    }
    cell.modifier.remove(Modifier::REVERSED);
    cell.set_fg(theme.bg_base);
    cell.set_bg(theme.text_primary);
}

fn slice_display_columns(text: &str, columns: Range<u16>) -> String {
    let start = display_column_to_byte(text, usize::from(columns.start), false);
    let end = display_column_to_byte(text, usize::from(columns.end), true);
    text.get(start..end).unwrap_or_default().to_string()
}

fn expand_display_columns(text: &str, columns: Range<u16>) -> Range<u16> {
    let start_byte = display_column_to_byte(text, usize::from(columns.start), false);
    let end_byte = display_column_to_byte(text, usize::from(columns.end), true);
    let start = u16::try_from(line_width(&text[..start_byte])).unwrap_or(u16::MAX);
    let end = u16::try_from(line_width(&text[..end_byte])).unwrap_or(u16::MAX);
    start..end
}

fn display_column_to_byte(text: &str, column: usize, include_cell: bool) -> usize {
    for (byte, character) in text.char_indices() {
        let end = byte + character.len_utf8();
        let next_width = line_width(&text[..end]);
        if column < next_width {
            return if include_cell { end } else { byte };
        }
        if column == next_width {
            return end;
        }
    }
    text.len()
}

fn line_width(text: &str) -> usize {
    Line::from(text).width()
}

#[cfg(test)]
#[path = "selection_tests.rs"]
mod tests;
