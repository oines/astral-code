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
use std::time::Duration;
use std::time::Instant;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::AstralTheme;
use super::ScrollbackViewport;
use super::transcript::TranscriptLayout;
use super::transcript::TranscriptSelectableLine as SelectableLine;
use super::transcript::TranscriptSelectableRange as SelectableRange;

const DEFAULT_SELECTION_HIGHLIGHT_DURATION: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SelectionPoint {
    range: usize,
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SelectionModel {
    ranges: Vec<SelectableRange>,
}

impl SelectionModel {
    fn from_layout(layout: &TranscriptLayout) -> Self {
        Self {
            ranges: layout.selectable_ranges.clone(),
        }
    }

    fn hit_test(&self, frame: SelectionFrame, column: u16, row: u16) -> Option<SelectionPoint> {
        if !frame.area.contains((column, row).into()) {
            return None;
        }
        let line_index = frame
            .viewport
            .first_visible_line
            .saturating_add(usize::from(row.saturating_sub(frame.area.y)));
        let relative_column = column.saturating_sub(frame.area.x);
        self.ranges
            .iter()
            .enumerate()
            .find_map(|(range_index, range)| {
                let line = range
                    .lines
                    .iter()
                    .find(|line| line.line == line_index && !line.columns.is_empty())?;
                Some(SelectionPoint {
                    range: range_index,
                    line: line_index,
                    column: clamp_column(&line.columns, relative_column),
                })
            })
    }

    fn hit_test_nearest(
        &self,
        frame: SelectionFrame,
        anchor: SelectionPoint,
        column: u16,
        row: u16,
    ) -> Option<SelectionPoint> {
        let range = self.ranges.get(anchor.range)?;
        let relative_column = column.saturating_sub(frame.area.x);
        let mut best: Option<((u16, u16), usize, SelectionPoint)> = None;
        for line in &range.lines {
            if line.columns.is_empty() {
                continue;
            }
            if !(frame.viewport.first_visible_line..frame.viewport.end_visible_line)
                .contains(&line.line)
            {
                continue;
            }
            let screen_row = frame.area.y.saturating_add(
                u16::try_from(line.line.saturating_sub(frame.viewport.first_visible_line))
                    .unwrap_or(u16::MAX),
            );
            let column_distance = distance_to_columns(&line.columns, relative_column);
            let key = (screen_row.abs_diff(row), column_distance);
            let anchor_distance = line.line.abs_diff(anchor.line);
            let point = SelectionPoint {
                range: anchor.range,
                line: line.line,
                column: clamp_column(&line.columns, relative_column),
            };
            if best
                .as_ref()
                .is_none_or(|(best_key, best_anchor_distance, _)| {
                    key < *best_key || (key == *best_key && anchor_distance > *best_anchor_distance)
                })
            {
                best = Some((key, anchor_distance, point));
            }
        }
        best.map(|(_, _, point)| point)
    }
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
/// The per-frame selection model mirrors Grok's `(entry, range)` boundary:
/// drag heads stay inside the anchor block instead of sweeping unrelated
/// transcript rows.
#[derive(Debug, Default)]
pub(crate) struct ScrollbackSelection {
    frame: Option<SelectionFrame>,
    model: SelectionModel,
    pending: Option<SelectionPoint>,
    active: Option<SelectionRange>,
    persistent: Option<SelectionRange>,
    persistent_created_at: Option<Instant>,
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
                self.persistent_created_at = None;
                ScrollbackSelectionAction::Redraw
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some(anchor) = self
                    .pending
                    .or_else(|| self.active.map(|range| range.anchor))
                else {
                    return ScrollbackSelectionAction::Ignored;
                };
                let Some(head) = self.hit_test_nearest(anchor, mouse.column, mouse.row) else {
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
                if let Some(head) = self.hit_test_nearest(range.anchor, mouse.column, mouse.row) {
                    range.head = head;
                }
                self.pending = None;
                let Some(text) = self.copy_text(range).filter(|text| !text.is_empty()) else {
                    return ScrollbackSelectionAction::Redraw;
                };
                self.persistent = Some(range);
                self.persistent_created_at = Some(Instant::now());
                ScrollbackSelectionAction::Copy(text)
            }
            _ => ScrollbackSelectionAction::Ignored,
        }
    }

    pub(crate) fn render(
        &mut self,
        layout: &TranscriptLayout,
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
        self.model = SelectionModel::from_layout(layout);
        if self.is_tracking() {
            self.lines = layout.lines.iter().map(Line::to_string).collect();
        } else {
            self.lines.clear();
        }
        if let Some(range) = self.active.or(self.persistent) {
            render_selection_overlay(
                range,
                &self.model,
                &self.lines,
                viewport,
                area,
                buffer,
                theme,
            );
        }
    }

    pub(crate) fn clear(&mut self) -> bool {
        let changed = self.pending.take().is_some()
            || self.active.take().is_some()
            || self.persistent.take().is_some();
        self.persistent_created_at = None;
        if changed {
            self.lines.clear();
        }
        changed
    }

    pub(crate) fn clear_persistent(&mut self) -> bool {
        let changed = self.persistent.take().is_some();
        self.persistent_created_at = None;
        if changed && self.pending.is_none() && self.active.is_none() {
            self.lines.clear();
        }
        changed
    }

    pub(crate) fn expiry(&self) -> Option<Instant> {
        self.persistent_created_at
            .and_then(|created| created.checked_add(DEFAULT_SELECTION_HIGHLIGHT_DURATION))
    }

    pub(crate) fn expire_if_due(&mut self, now: Instant) -> bool {
        if self.expiry().is_some_and(|expiry| expiry <= now) {
            return self.clear_persistent();
        }
        false
    }

    fn is_tracking(&self) -> bool {
        self.pending.is_some() || self.active.is_some() || self.persistent.is_some()
    }

    fn hit_test(&self, column: u16, row: u16) -> Option<SelectionPoint> {
        let frame = self.frame?;
        self.model.hit_test(frame, column, row)
    }

    fn hit_test_nearest(
        &self,
        anchor: SelectionPoint,
        column: u16,
        row: u16,
    ) -> Option<SelectionPoint> {
        let frame = self.frame?;
        self.model.hit_test_nearest(frame, anchor, column, row)
    }

    fn copy_text(&self, range: SelectionRange) -> Option<String> {
        let (start, end) = range.normalized();
        if start.range != end.range {
            return None;
        }
        let selectable_range = self.model.ranges.get(start.range)?;
        let mut selected = String::new();
        let mut first = true;
        for selectable_line in selectable_range
            .lines
            .iter()
            .filter(|line| (start.line..=end.line).contains(&line.line))
        {
            if !first {
                selected.push_str(selectable_line.joiner_to_previous.as_str());
            }
            first = false;
            let line_index = selectable_line.line;
            let line = self.lines.get(line_index)?;
            let columns = if start.line == end.line {
                start.column..end.column.saturating_add(1)
            } else if line_index == start.line {
                start.column..selectable_line.columns.end
            } else if line_index == end.line {
                selectable_line.columns.start..end.column.saturating_add(1)
            } else {
                selectable_line.columns.clone()
            };
            selected.push_str(&slice_display_columns(line, columns));
        }
        (!first).then_some(selected)
    }
}

fn compare_points(left: SelectionPoint, right: SelectionPoint) -> Ordering {
    (left.range, left.line, left.column).cmp(&(right.range, right.line, right.column))
}

fn clamp_column(columns: &Range<u16>, column: u16) -> u16 {
    column.clamp(columns.start, columns.end.saturating_sub(1))
}

fn distance_to_columns(columns: &Range<u16>, column: u16) -> u16 {
    if column < columns.start {
        columns.start - column
    } else if column >= columns.end {
        column.saturating_sub(columns.end.saturating_sub(1))
    } else {
        0
    }
}

fn render_selection_overlay(
    range: SelectionRange,
    model: &SelectionModel,
    lines: &[String],
    viewport: ScrollbackViewport,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    let (start, end) = range.normalized();
    let Some(selectable_range) = model.ranges.get(start.range) else {
        return;
    };
    for selectable_line in selectable_range
        .lines
        .iter()
        .filter(|line| (start.line..=end.line).contains(&line.line))
    {
        let line_index = selectable_line.line;
        if !(viewport.first_visible_line..viewport.end_visible_line).contains(&line_index) {
            continue;
        }
        let Some(line) = lines.get(line_index) else {
            continue;
        };
        let columns = expand_display_columns(line, selected_columns(start, end, selectable_line));
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
    line: &SelectableLine,
) -> Range<u16> {
    let line_index = line.line;
    if start.line == end.line {
        start.column.max(line.columns.start)..end.column.saturating_add(1).min(line.columns.end)
    } else if line_index == start.line {
        start.column.max(line.columns.start)..line.columns.end
    } else if line_index == end.line {
        line.columns.start..end.column.saturating_add(1).min(line.columns.end)
    } else {
        line.columns.clone()
    }
}

pub(super) fn apply_selection_highlight(theme: AstralTheme, cell: &mut ratatui::buffer::Cell) {
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
    let mut width: usize = 0;
    for (byte, grapheme) in text.grapheme_indices(true) {
        let end = byte + grapheme.len();
        let next_width = width.saturating_add(UnicodeWidthStr::width(grapheme));
        if column < next_width {
            return if include_cell { end } else { byte };
        }
        if column == next_width {
            return end;
        }
        width = next_width;
    }
    text.len()
}

fn line_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

#[cfg(test)]
#[path = "selection_tests.rs"]
mod tests;
