//! Retained viewport state over the shared conversation surface.
//!
//! The viewport owns navigation policy only. It never renders or reprojects
//! transcript items, which keeps inline and fullscreen content identical.

use crate::ConversationSurface;
use crate::SurfaceAnchor;
use crate::SurfaceNode;
use crate::SurfaceNodeId;

/// Vertical movement through the conversation surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
}

/// Scroll, follow-bottom, selection, and hover state for a rendered surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceViewport {
    top: usize,
    height: u16,
    follow_bottom: bool,
    anchor: Option<SurfaceAnchor>,
    selected: Option<SurfaceNodeId>,
    hovered: Option<SurfaceNodeId>,
}

impl Default for SurfaceViewport {
    fn default() -> Self {
        Self {
            top: 0,
            height: 0,
            follow_bottom: true,
            anchor: None,
            selected: None,
            hovered: None,
        }
    }
}

impl SurfaceViewport {
    pub fn top(&self) -> usize {
        self.top
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    pub fn end(&self, surface: &ConversationSurface) -> usize {
        self.top
            .saturating_add(usize::from(self.height))
            .min(surface.row_count())
    }

    pub fn visible_rows(&self, surface: &ConversationSurface) -> std::ops::Range<usize> {
        self.top..self.end(surface)
    }

    pub fn is_following_bottom(&self) -> bool {
        self.follow_bottom
    }

    pub fn selected(&self) -> Option<SurfaceNodeId> {
        self.selected
    }

    pub fn hovered(&self) -> Option<SurfaceNodeId> {
        self.hovered
    }

    /// Reconcile retained state against a new surface or terminal height.
    ///
    /// Follow mode pins to the newest row. Manual mode resolves the semantic
    /// anchor captured at the previous top row, so width-dependent wrapping or
    /// earlier transcript growth cannot move the user's reading position.
    pub fn prepare(&mut self, surface: &ConversationSurface, height: u16) {
        self.height = height;
        self.retain_targets(surface);
        if self.follow_bottom {
            self.top = max_top(surface, height);
        } else if let Some(row) = self
            .anchor
            .and_then(|anchor| surface.row_for_anchor(anchor))
        {
            self.top = row.min(max_top(surface, height));
        } else {
            self.top = self.top.min(max_top(surface, height));
        }
        self.capture_anchor(surface);
    }

    pub fn scroll_rows(
        &mut self,
        surface: &ConversationSurface,
        direction: ScrollDirection,
        rows: usize,
    ) -> bool {
        let before_top = self.top;
        let before_follow = self.follow_bottom;
        match direction {
            ScrollDirection::Up => {
                self.top = self.top.saturating_sub(rows);
                self.follow_bottom = false;
            }
            ScrollDirection::Down => {
                let maximum = max_top(surface, self.height);
                self.top = self.top.saturating_add(rows).min(maximum);
                if rows > 0 && self.top == before_top && self.top == maximum {
                    self.follow_bottom = true;
                }
            }
        }
        self.capture_anchor(surface);
        self.top != before_top || self.follow_bottom != before_follow
    }

    pub fn scroll_page(
        &mut self,
        surface: &ConversationSurface,
        direction: ScrollDirection,
    ) -> bool {
        let changed = self.scroll_rows(
            surface,
            direction,
            usize::from(self.height.saturating_sub(2).max(1)),
        );
        let selected_before = self.selected;
        let edge_row = match direction {
            ScrollDirection::Up => self.top,
            ScrollDirection::Down => self.end(surface).saturating_sub(1),
        };
        if let Some(node) = surface.node_at_row(edge_row) {
            self.selected = Some(node.id());
        }
        changed || self.selected != selected_before
    }

    pub fn scroll_to_top(&mut self, surface: &ConversationSurface) -> bool {
        let before_top = self.top;
        let before_follow = self.follow_bottom;
        self.top = 0;
        self.follow_bottom = false;
        self.capture_anchor(surface);
        self.top != before_top || self.follow_bottom != before_follow
    }

    pub fn scroll_to_bottom(&mut self, surface: &ConversationSurface) -> bool {
        let before = self.top;
        self.top = max_top(surface, self.height);
        self.follow_bottom = true;
        self.capture_anchor(surface);
        self.top != before
    }

    /// Move to an explicit virtual row without enabling tail follow.
    ///
    /// This is the semantic operation used by a scrollbar drag. Reaching the
    /// final row through a drag remains a manual reading position; an explicit
    /// bottom command or downward overscroll is what resumes follow mode.
    pub fn scroll_to_row(&mut self, surface: &ConversationSurface, row: usize) -> bool {
        let before_top = self.top;
        let before_follow = self.follow_bottom;
        self.top = row.min(max_top(surface, self.height));
        self.follow_bottom = false;
        self.capture_anchor(surface);
        self.top != before_top || self.follow_bottom != before_follow
    }

    /// Map one zero-based screen row inside the transcript viewport back to
    /// the shared surface and update hover state.
    pub fn hover_screen_row(
        &mut self,
        surface: &ConversationSurface,
        screen_row: u16,
    ) -> Option<SurfaceNodeId> {
        self.hovered = (screen_row < self.height)
            .then(|| self.top.saturating_add(usize::from(screen_row)))
            .and_then(|row| surface.node_at_row(row))
            .map(SurfaceNode::id);
        self.hovered
    }

    pub fn clear_hover(&mut self) {
        self.hovered = None;
    }

    pub fn select_last(&mut self, surface: &ConversationSurface) -> bool {
        let next = surface
            .nodes()
            .iter()
            .rev()
            .find(|node| !node.rows().is_empty())
            .map(SurfaceNode::id);
        self.select_node(surface, next)
    }

    pub fn select_first(&mut self, surface: &ConversationSurface) -> bool {
        let next = surface
            .nodes()
            .iter()
            .find(|node| !node.rows().is_empty())
            .map(SurfaceNode::id);
        self.select_node(surface, next)
    }

    /// Select a stable surface node and reveal it when necessary.
    pub fn select_node(
        &mut self,
        surface: &ConversationSurface,
        selected: Option<SurfaceNodeId>,
    ) -> bool {
        let selected = selected.filter(|id| {
            surface
                .node(*id)
                .is_some_and(|node| !node.rows().is_empty())
        });
        if selected == self.selected {
            return false;
        }
        self.selected = selected;
        if let Some(selected) = selected {
            self.ensure_node_visible(surface, selected);
        }
        true
    }

    pub fn move_selection(
        &mut self,
        surface: &ConversationSurface,
        direction: ScrollDirection,
    ) -> bool {
        let nodes = surface.nodes();
        let next = match (self.selected, direction) {
            (None, ScrollDirection::Up) => nodes.iter().rev().find(|node| !node.rows().is_empty()),
            (None, ScrollDirection::Down) => nodes.iter().find(|node| !node.rows().is_empty()),
            (Some(selected), ScrollDirection::Up) => nodes
                .iter()
                .position(|node| node.id() == selected)
                .and_then(|index| {
                    nodes[..index]
                        .iter()
                        .rev()
                        .find(|node| !node.rows().is_empty())
                }),
            (Some(selected), ScrollDirection::Down) => nodes
                .iter()
                .position(|node| node.id() == selected)
                .and_then(|index| {
                    nodes[index.saturating_add(1)..]
                        .iter()
                        .find(|node| !node.rows().is_empty())
                }),
        }
        .map(SurfaceNode::id);
        if let Some(next) = next {
            return self.select_node(surface, Some(next));
        }
        if direction == ScrollDirection::Down && self.selected.is_some() {
            let was_following = self.follow_bottom;
            return self.scroll_to_bottom(surface) || !was_following;
        }
        false
    }

    pub fn clear_selection(&mut self) -> bool {
        self.selected.take().is_some()
    }

    fn ensure_node_visible(&mut self, surface: &ConversationSurface, id: SurfaceNodeId) {
        let Some(node) = surface.node(id) else {
            return;
        };
        let rows = node.rows();
        let height = usize::from(self.height);
        let before = self.top;
        if rows.start < self.top {
            self.top = rows.start;
        } else if rows.end > self.end(surface) {
            self.top = if rows.len() > height {
                rows.start
            } else {
                rows.end.saturating_sub(height)
            };
        }
        self.top = self.top.min(max_top(surface, self.height));
        if self.top != before {
            self.follow_bottom = false;
        }
        self.capture_anchor(surface);
    }

    fn retain_targets(&mut self, surface: &ConversationSurface) {
        self.selected = self.selected.filter(|id| {
            surface
                .node(*id)
                .is_some_and(|node| !node.rows().is_empty())
        });
        self.hovered = self.hovered.filter(|id| {
            surface
                .node(*id)
                .is_some_and(|node| !node.rows().is_empty())
        });
    }

    fn capture_anchor(&mut self, surface: &ConversationSurface) {
        self.anchor = surface.anchor_at_row(self.top);
    }
}

fn max_top(surface: &ConversationSurface, height: u16) -> usize {
    surface.row_count().saturating_sub(usize::from(height))
}

#[cfg(test)]
#[path = "viewport_tests.rs"]
mod tests;
