// Derived from Grok Build's character-precise BlockViewer selection at
// commit 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::BlockViewerState;
use super::TextDrag;
use super::TextEndpoint;

impl BlockViewerState {
    pub(crate) fn clear_text_drag(&mut self) -> bool {
        self.text_drag.take().is_some()
    }

    pub(super) fn start_text_drag(&mut self, column: u16, row: u16) -> bool {
        let Some(endpoint) = self.screen_to_endpoint(column, row) else {
            return false;
        };
        self.pause_follow();
        self.clear_visual_selection();
        self.text_drag = Some(TextDrag {
            anchor: endpoint,
            head: endpoint,
        });
        true
    }

    pub(super) fn update_text_drag(&mut self, column: u16, row: u16) -> bool {
        if self.text_drag.is_none() {
            return false;
        }
        let Some(area) = self.content_area else {
            return false;
        };
        if row < area.y {
            let distance = isize::try_from(area.y.saturating_sub(row)).unwrap_or(isize::MAX);
            self.scroll_by(-distance.clamp(1, 5));
        } else if row >= area.bottom() {
            let distance = isize::try_from(row.saturating_sub(area.bottom()).saturating_add(1))
                .unwrap_or(isize::MAX);
            self.scroll_by(distance.clamp(1, 5));
        }
        let column = column.clamp(area.x, area.right().saturating_sub(1));
        let row = row.clamp(area.y, area.bottom().saturating_sub(1));
        let Some(endpoint) = self.screen_to_endpoint(column, row) else {
            return true;
        };
        if let Some(drag) = self.text_drag.as_mut() {
            drag.head = endpoint;
        }
        true
    }

    pub(super) fn finish_text_drag(&mut self, column: u16, row: u16) -> Option<String> {
        let mut drag = self.text_drag.take()?;
        if let Some(area) = self.content_area {
            let column = column.clamp(area.x, area.right().saturating_sub(1));
            let row = row.clamp(area.y, area.bottom().saturating_sub(1));
            if let Some(endpoint) = self.screen_to_endpoint(column, row) {
                drag.head = endpoint;
            }
        }
        drag.is_non_empty()
            .then(|| self.text_for_drag(drag))
            .flatten()
    }

    pub(crate) fn text_drag_columns(&self, visible_row: usize) -> Option<Range<u16>> {
        let drag = self.text_drag?;
        if !drag.is_non_empty() {
            return None;
        }
        let (start, end) = drag.ordered();
        let physical_row = *self.visible_row_indices.get(visible_row)?;
        let geometry = *self.row_geometry.get(physical_row)?;
        if !(start.item..=end.item).contains(&geometry.item) {
            return None;
        }
        let end_column = self
            .logical_lines
            .get(end.item)
            .map_or(end.column, |text| col_past_grapheme(text, end.column));
        let selected = if start.item == end.item {
            start.column..end_column
        } else if geometry.item == start.item {
            start.column..u16::MAX
        } else if geometry.item == end.item {
            0..end_column
        } else {
            0..u16::MAX
        };
        let start = selected.start.max(geometry.logical_start);
        let end = selected.end.min(geometry.logical_end);
        (start < end).then(|| {
            start.saturating_sub(geometry.logical_start)..end.saturating_sub(geometry.logical_start)
        })
    }

    fn screen_to_endpoint(&self, column: u16, row: u16) -> Option<TextEndpoint> {
        let area = self.content_area?;
        if !area.contains((column, row).into()) {
            return None;
        }
        let visible_row = self
            .scroll_offset
            .saturating_add(usize::from(row.saturating_sub(area.y)));
        let physical_row = *self.visible_row_indices.get(visible_row)?;
        let geometry = *self.row_geometry.get(physical_row)?;
        let row_text = self.rendered_rows.get(physical_row)?;
        let row_width =
            u16::try_from(UnicodeWidthStr::width(row_text.as_str())).unwrap_or(u16::MAX);
        let column_in_row = column.saturating_sub(area.x);
        let has_later_subrow = self
            .row_geometry
            .get(physical_row.saturating_add(1))
            .is_some_and(|next| next.item == geometry.item);
        let logical_column = if column_in_row >= row_width && has_later_subrow {
            self.logical_lines
                .get(geometry.item)
                .map(|text| {
                    u16::try_from(UnicodeWidthStr::width(text.as_str())).unwrap_or(u16::MAX)
                })
                .unwrap_or(geometry.logical_end)
        } else {
            geometry
                .logical_start
                .saturating_add(column_in_row.min(row_width))
        };
        Some(TextEndpoint {
            item: geometry.item,
            column: logical_column,
        })
    }

    fn text_for_drag(&self, drag: TextDrag) -> Option<String> {
        let (start, end) = drag.ordered();
        let mut output = String::new();
        for item in start.item..=end.item {
            let text = self.logical_lines.get(item)?;
            let selected = if start.item == end.item {
                start.column..col_past_grapheme(text, end.column)
            } else if item == start.item {
                start.column..u16::MAX
            } else if item == end.item {
                0..col_past_grapheme(text, end.column)
            } else {
                0..u16::MAX
            };
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(&slice_display_columns(text, selected));
        }
        (!output.is_empty()).then_some(output)
    }
}

fn col_past_grapheme(text: &str, column: u16) -> u16 {
    let mut current = 0u16;
    for grapheme in text.graphemes(true) {
        let width = u16::try_from(UnicodeWidthStr::width(grapheme))
            .unwrap_or(u16::MAX)
            .max(1);
        let next = current.saturating_add(width);
        if next > column {
            return next;
        }
        current = next;
    }
    current
}

fn slice_display_columns(text: &str, columns: Range<u16>) -> String {
    let start = display_column_to_byte(text, usize::from(columns.start), false);
    let end = display_column_to_byte(text, usize::from(columns.end), true);
    text.get(start..end).unwrap_or_default().to_string()
}

fn display_column_to_byte(text: &str, column: usize, include_cell: bool) -> usize {
    let mut width = 0usize;
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
