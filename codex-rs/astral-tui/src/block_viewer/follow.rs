// Derived from Grok Build's ListPane follow/navigation transitions at
// commit 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).

use super::ViewerState;

impl ViewerState {
    pub(crate) fn toggle_follow(&mut self) -> bool {
        if !self.follow_enabled {
            return false;
        }
        if self.follow_mode {
            self.pause_follow()
        } else {
            self.engage_follow()
        }
    }

    pub(super) fn pause_follow(&mut self) -> bool {
        if !self.follow_mode {
            return false;
        }
        self.follow_mode = false;
        let last_visible_row = self
            .scroll_offset
            .saturating_add(self.page_size)
            .min(self.total_rows)
            .saturating_sub(1);
        self.select_item_at_row(last_visible_row);
        self.reset_edge_state();
        true
    }

    pub(super) fn engage_follow(&mut self) -> bool {
        if !self.follow_enabled {
            return false;
        }
        let changed = !self.follow_mode
            || self.scroll_offset != self.max_scroll_offset
            || self.selected_item.is_some();
        self.follow_mode = true;
        self.selected_item = None;
        self.visual_anchor = None;
        self.scroll_offset = self.max_scroll_offset;
        self.reset_edge_state();
        changed
    }

    pub(super) fn push_past_content_edge(&mut self) -> bool {
        if !self.follow_engagement_allowed() {
            return false;
        }
        if self.at_content_edge {
            self.engage_follow()
        } else {
            self.at_content_edge = true;
            false
        }
    }

    pub(super) fn follow_engagement_allowed(&self) -> bool {
        self.follow_enabled && self.visual_anchor.is_none() && self.text_drag.is_none()
    }

    pub(super) fn reset_edge_state(&mut self) {
        self.at_content_edge = false;
        self.mouse_overscroll = 0;
    }
}
