// Derived from Grok Build's ListPane navigation and stable-item layout at
// commit 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).

use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;

use super::BlockViewerMouseAction;
use super::BlockViewerState;

impl BlockViewerState {
    pub(crate) fn scroll_by(&mut self, lines: isize) -> bool {
        let old_offset = self.scroll_offset;
        let selected_screen_row = self.selected_item_screen_row().unwrap_or(0);
        let next = self
            .scroll_offset
            .saturating_add_signed(lines)
            .min(self.max_scroll_offset);
        if next == old_offset {
            return false;
        }
        self.scroll_offset = next;
        self.select_item_at_row(
            next.saturating_add(selected_screen_row)
                .min(self.total_rows.saturating_sub(1)),
        );
        true
    }

    pub(crate) fn scroll_page(&mut self, pages: isize) -> bool {
        let lines = isize::try_from(self.page_size)
            .unwrap_or(isize::MAX)
            .saturating_mul(pages);
        self.scroll_by(lines)
    }

    pub(crate) fn scroll_half_page(&mut self, pages: isize) -> bool {
        let lines = isize::try_from((self.page_size / 2).max(1))
            .unwrap_or(isize::MAX)
            .saturating_mul(pages);
        self.scroll_by(lines)
    }

    pub(crate) fn select_by(&mut self, items: isize) -> bool {
        let Some(selected) = self.selected_item else {
            return false;
        };
        let next = selected
            .saturating_add_signed(items)
            .min(self.visible_item_indices.len().saturating_sub(1));
        if next == selected {
            return false;
        }
        self.selected_item = Some(next);
        self.reveal_selected_item();
        true
    }

    pub(crate) fn scroll_to_start(&mut self) -> bool {
        let changed = self.scroll_offset != 0 || self.selected_item != Some(0);
        if !changed {
            return false;
        }
        self.scroll_offset = 0;
        self.selected_item = (!self.visible_item_indices.is_empty()).then_some(0);
        changed
    }

    pub(crate) fn scroll_to_end(&mut self) -> bool {
        let last_item = self.visible_item_indices.len().checked_sub(1);
        let changed =
            self.scroll_offset != self.max_scroll_offset || self.selected_item != last_item;
        if !changed {
            return false;
        }
        self.scroll_offset = self.max_scroll_offset;
        self.selected_item = last_item;
        changed
    }

    pub(crate) fn handle_mouse(&mut self, mouse: MouseEvent) -> BlockViewerMouseAction {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let position = (mouse.column, mouse.row).into();
                if self
                    .close_button
                    .is_some_and(|area| area.contains(position))
                    || self.popup_area.is_some_and(|area| !area.contains(position))
                {
                    self.clear_text_drag();
                    BlockViewerMouseAction::Close
                } else if let Some(area) = self.content_area
                    && area.contains(position)
                {
                    if self.start_text_drag(mouse.column, mouse.row) {
                        BlockViewerMouseAction::Redraw
                    } else {
                        BlockViewerMouseAction::Ignored
                    }
                } else {
                    self.clear_text_drag();
                    BlockViewerMouseAction::Ignored
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.update_text_drag(mouse.column, mouse.row) {
                    BlockViewerMouseAction::Redraw
                } else {
                    BlockViewerMouseAction::Ignored
                }
            }
            MouseEventKind::Up(MouseButton::Left) => self
                .finish_text_drag(mouse.column, mouse.row)
                .map_or(BlockViewerMouseAction::Redraw, BlockViewerMouseAction::Copy),
            MouseEventKind::Moved => {
                let hovered = self
                    .close_button
                    .is_some_and(|area| area.contains((mouse.column, mouse.row).into()));
                if hovered == self.close_hovered {
                    BlockViewerMouseAction::Ignored
                } else {
                    self.close_hovered = hovered;
                    BlockViewerMouseAction::Redraw
                }
            }
            MouseEventKind::ScrollUp => {
                self.clear_visual_selection();
                self.scroll_by(-3);
                BlockViewerMouseAction::Redraw
            }
            MouseEventKind::ScrollDown => {
                self.clear_visual_selection();
                self.scroll_by(3);
                BlockViewerMouseAction::Redraw
            }
            MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight
            | MouseEventKind::Up(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Drag(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Down(MouseButton::Right | MouseButton::Middle) => {
                self.clear_text_drag();
                BlockViewerMouseAction::Ignored
            }
        }
    }

    pub(super) fn reveal_selected_item(&mut self) {
        let Some(selected) = self.selected_item else {
            return;
        };
        let Some(rows) = self.visible_item_row_range(selected) else {
            return;
        };
        if rows.len() > self.page_size {
            if rows.start < self.scroll_offset
                || rows.start >= self.scroll_offset.saturating_add(self.page_size)
            {
                self.scroll_offset = rows.start.min(self.max_scroll_offset);
            }
            return;
        }
        if rows.start < self.scroll_offset {
            self.scroll_offset = rows.start;
        } else if rows.end > self.scroll_offset.saturating_add(self.page_size) {
            self.scroll_offset = rows
                .end
                .saturating_sub(self.page_size)
                .min(self.max_scroll_offset);
        }
    }

    pub(super) fn select_physical_item(&mut self, physical: usize) {
        let Some(item) = self.visible_item_position(physical) else {
            return;
        };
        self.selected_item = Some(item);
        self.reveal_selected_item();
    }

    fn select_item_at_row(&mut self, row: usize) {
        let Some(physical) = self.visible_row_physical_item(row) else {
            return;
        };
        self.selected_item = self.visible_item_position(physical);
    }

    pub(super) fn selected_physical_item(&self) -> Option<usize> {
        self.selected_item
            .and_then(|selected| self.visible_item_indices.get(selected))
            .copied()
    }

    pub(super) fn visual_anchor_physical_item(&self) -> Option<usize> {
        self.visual_anchor
            .and_then(|anchor| self.visible_item_indices.get(anchor))
            .copied()
    }

    pub(super) fn selected_item_screen_row(&self) -> Option<usize> {
        let selected = self.selected_item?;
        let rows = self.visible_item_row_range(selected)?;
        Some(
            rows.start
                .saturating_sub(self.scroll_offset)
                .min(self.page_size.saturating_sub(1)),
        )
    }

    pub(super) fn visible_item_position(&self, physical: usize) -> Option<usize> {
        self.visible_item_indices.binary_search(&physical).ok()
    }

    pub(super) fn visible_row_physical_item(&self, row: usize) -> Option<usize> {
        let physical_row = *self.visible_row_indices.get(row)?;
        self.row_geometry
            .get(physical_row)
            .map(|geometry| geometry.item)
    }

    fn visible_item_row_range(&self, item: usize) -> Option<std::ops::Range<usize>> {
        let physical = *self.visible_item_indices.get(item)?;
        let start = self.visible_row_indices.iter().position(|row| {
            self.row_geometry
                .get(*row)
                .is_some_and(|geometry| geometry.item == physical)
        })?;
        let length = self.visible_row_indices[start..]
            .iter()
            .take_while(|row| {
                self.row_geometry
                    .get(**row)
                    .is_some_and(|geometry| geometry.item == physical)
            })
            .count();
        Some(start..start.saturating_add(length))
    }

    pub(super) fn refresh_visible_rows(&mut self) {
        let selected_physical = self.selected_physical_item();
        let anchor_physical = self.visual_anchor_physical_item();
        let selected_screen_row = self.selected_item_screen_row();
        self.rebuild_visible_rows(selected_physical, anchor_physical, selected_screen_row);
    }

    pub(super) fn rebuild_visible_rows(
        &mut self,
        selected_physical: Option<usize>,
        anchor_physical: Option<usize>,
        selected_screen_row: Option<usize>,
    ) {
        let fallback_item = self.selected_item.unwrap_or(0);
        self.visible_item_indices = if self.matcher.filter_active() {
            self.matcher.match_lines().to_vec()
        } else {
            (0..self.logical_lines.len()).collect()
        };
        self.visible_row_indices = self
            .row_geometry
            .iter()
            .enumerate()
            .filter_map(|(row, geometry)| {
                self.visible_item_indices
                    .binary_search(&geometry.item)
                    .is_ok()
                    .then_some(row)
            })
            .collect();
        self.total_rows = self.visible_row_indices.len();
        self.max_scroll_offset = self.total_rows.saturating_sub(self.page_size);
        self.selected_item = match self.visible_item_indices.len() {
            0 => None,
            item_count => selected_physical
                .and_then(|physical| self.visible_item_position(physical))
                .or_else(|| Some(fallback_item.min(item_count - 1))),
        };
        self.visual_anchor =
            anchor_physical.and_then(|physical| self.visible_item_position(physical));
        if let Some(screen_row) = selected_screen_row
            && let Some(selected) = self.selected_item
            && let Some(rows) = self.visible_item_row_range(selected)
        {
            self.scroll_offset = rows
                .start
                .saturating_sub(screen_row)
                .min(self.max_scroll_offset);
        } else {
            self.scroll_offset = self.scroll_offset.min(self.max_scroll_offset);
        }
    }

    pub(super) fn next_match_line(&self) -> Option<usize> {
        let selected_item = self.selected_item.unwrap_or(0);
        if self.matcher.filter_active() {
            if self.visible_item_indices.is_empty() {
                return None;
            }
            return self
                .visible_item_indices
                .get((selected_item + 1) % self.visible_item_indices.len())
                .copied();
        }
        self.matcher
            .next_match(self.selected_physical_item().unwrap_or(0))
    }

    pub(super) fn previous_match_line(&self) -> Option<usize> {
        let selected_item = self.selected_item.unwrap_or(0);
        if self.matcher.filter_active() {
            let item_count = self.visible_item_indices.len();
            if item_count == 0 {
                return None;
            }
            let previous = selected_item.checked_sub(1).unwrap_or(item_count - 1);
            return self.visible_item_indices.get(previous).copied();
        }
        self.matcher
            .previous_match(self.selected_physical_item().unwrap_or(0))
    }
}
