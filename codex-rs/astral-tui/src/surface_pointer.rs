//! Shared pointer gestures for a retained conversation surface.
//!
//! Hosts own transcript semantics. This controller only maps terminal mouse
//! coordinates to stable surface nodes, viewport movement, and a double-click
//! activation that the host may interpret as fold/unfold.

use std::time::Duration;
use std::time::Instant;

use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::layout::Rect;

use crate::ConversationSurface;
use crate::ScrollDirection;
use crate::SurfaceNodeId;
use crate::SurfaceViewport;

const MULTI_CLICK_TIMEOUT: Duration = Duration::from_millis(300);
const WHEEL_ROWS: usize = 3;

/// Result of routing one pointer event over a conversation surface.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SurfacePointerOutcome {
    changed: bool,
    activated: Option<SurfaceNodeId>,
}

impl SurfacePointerOutcome {
    pub fn changed(self) -> bool {
        self.changed
    }

    /// Stable node activated by a double-click.
    pub fn activated(self) -> Option<SurfaceNodeId> {
        self.activated
    }
}

/// Retained click, hover, wheel, and scrollbar state shared by surface hosts.
#[derive(Debug, Default)]
pub struct SurfacePointer {
    area: Option<Rect>,
    pending_click: Option<SurfaceNodeId>,
    last_click: Option<(SurfaceNodeId, Instant)>,
    scrollbar_dragging: bool,
}

impl SurfacePointer {
    /// Reconcile retained gestures after resize or surface replacement.
    pub fn prepare(&mut self, area: Rect, surface: &ConversationSurface) {
        if self.area != Some(area) {
            self.area = Some(area);
            self.cancel_gesture();
            return;
        }
        self.pending_click = self
            .pending_click
            .filter(|node| surface.node(*node).is_some());
        self.last_click = self
            .last_click
            .filter(|click| surface.node(click.0).is_some());
    }

    /// Drop all retained geometry and gestures when the source context changes.
    pub fn reset(&mut self) {
        self.area = None;
        self.cancel_gesture();
    }

    /// Cancel an in-flight click sequence without changing selection or hover.
    pub fn cancel_gesture(&mut self) {
        self.pending_click = None;
        self.last_click = None;
        self.scrollbar_dragging = false;
    }

    pub fn handle_event(
        &mut self,
        mouse: MouseEvent,
        now: Instant,
        area: Rect,
        surface: &ConversationSurface,
        viewport: &mut SurfaceViewport,
    ) -> SurfacePointerOutcome {
        self.prepare(area, surface);
        match mouse.kind {
            MouseEventKind::Moved => update_hover(mouse, area, surface, viewport),
            MouseEventKind::ScrollUp => {
                self.cancel_gesture();
                changed(viewport.scroll_rows(surface, ScrollDirection::Up, WHEEL_ROWS))
            }
            MouseEventKind::ScrollDown => {
                self.cancel_gesture();
                changed(viewport.scroll_rows(surface, ScrollDirection::Down, WHEEL_ROWS))
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if scrollbar_hit(mouse, area, surface) {
                    self.scrollbar_dragging = true;
                    self.pending_click = None;
                    self.last_click = None;
                    return changed(apply_scrollbar_row(mouse.row, area, surface, viewport));
                }
                self.pending_click = node_at_pointer(mouse, area, surface, viewport);
                if self.pending_click.is_none() {
                    self.last_click = None;
                }
                SurfacePointerOutcome::default()
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.scrollbar_dragging {
                    return changed(apply_scrollbar_row(mouse.row, area, surface, viewport));
                }
                self.pending_click = None;
                self.last_click = None;
                update_hover(mouse, area, surface, viewport)
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if self.scrollbar_dragging {
                    self.scrollbar_dragging = false;
                    return changed(apply_scrollbar_row(mouse.row, area, surface, viewport));
                }
                let Some(node) = self.pending_click.take() else {
                    return SurfacePointerOutcome::default();
                };
                self.complete_click(node, now, surface, viewport)
            }
            _ => SurfacePointerOutcome::default(),
        }
    }

    fn complete_click(
        &mut self,
        node: SurfaceNodeId,
        now: Instant,
        surface: &ConversationSurface,
        viewport: &mut SurfaceViewport,
    ) -> SurfacePointerOutcome {
        let changed = viewport.select_node(surface, Some(node));
        let double_click = self.last_click.is_some_and(|last| {
            last.0 == node && now.saturating_duration_since(last.1) < MULTI_CLICK_TIMEOUT
        });
        if double_click {
            self.last_click = None;
            return SurfacePointerOutcome {
                changed,
                activated: Some(node),
            };
        }
        self.last_click = Some((node, now));
        SurfacePointerOutcome {
            changed,
            activated: None,
        }
    }
}

fn update_hover(
    mouse: MouseEvent,
    area: Rect,
    surface: &ConversationSurface,
    viewport: &mut SurfaceViewport,
) -> SurfacePointerOutcome {
    let before = viewport.hovered();
    if contains(area, mouse) && !scrollbar_column(mouse.column, area) {
        viewport.hover_screen_row(surface, mouse.row.saturating_sub(area.y));
    } else {
        viewport.clear_hover();
    }
    changed(viewport.hovered() != before)
}

fn node_at_pointer(
    mouse: MouseEvent,
    area: Rect,
    surface: &ConversationSurface,
    viewport: &SurfaceViewport,
) -> Option<SurfaceNodeId> {
    if !contains(area, mouse) || scrollbar_column(mouse.column, area) {
        return None;
    }
    let virtual_row = viewport
        .top()
        .saturating_add(usize::from(mouse.row.saturating_sub(area.y)));
    surface
        .node_at_row(virtual_row)
        .map(super::surface::SurfaceNode::id)
}

fn scrollbar_hit(mouse: MouseEvent, area: Rect, surface: &ConversationSurface) -> bool {
    contains(area, mouse)
        && scrollbar_column(mouse.column, area)
        && surface.row_count() > usize::from(area.height)
}

fn scrollbar_column(column: u16, area: Rect) -> bool {
    area.width > 1 && column == area.right().saturating_sub(1)
}

fn apply_scrollbar_row(
    row: u16,
    area: Rect,
    surface: &ConversationSurface,
    viewport: &mut SurfaceViewport,
) -> bool {
    let height = usize::from(area.height);
    let total = surface.row_count();
    let maximum = total.saturating_sub(height);
    let click = usize::from(
        row.saturating_sub(area.y)
            .min(area.height.saturating_sub(1)),
    );
    let thumb_height = height
        .saturating_mul(height)
        .div_ceil(total)
        .clamp(1, height);
    let travel = height.saturating_sub(thumb_height);
    let thumb_top = click.saturating_sub(thumb_height / 2).min(travel);
    let target = maximum
        .saturating_mul(thumb_top)
        .checked_div(travel)
        .unwrap_or(0);
    viewport.scroll_to_row(surface, target)
}

fn contains(area: Rect, mouse: MouseEvent) -> bool {
    mouse.column >= area.x
        && mouse.column < area.right()
        && mouse.row >= area.y
        && mouse.row < area.bottom()
}

fn changed(changed: bool) -> SurfacePointerOutcome {
    SurfacePointerOutcome {
        changed,
        activated: None,
    }
}
