//! Retained fullscreen viewport and input policy over the shared conversation
//! surface. Runtime events and transcript projection remain in their owners.

use std::time::Duration;
use std::time::Instant;

use astral_tui_scrollback::EntryRenderOptions;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::ConversationState;
use crate::ConversationSurface;
use crate::EntryDisplayAction;
use crate::ScrollDirection;
use crate::SurfaceNodeId;
use crate::SurfaceRenderer;
use crate::SurfaceViewport;
use crate::VerbGroupDisplayAction;

const MULTI_CLICK_TIMEOUT: Duration = Duration::from_millis(300);
const FALLBACK_WHEEL_ROWS: i32 = 3;

/// Whether bare-letter Grok navigation is active while scrollback is focused.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollbackKeyMode {
    /// Letters return to the composer and are forwarded as text.
    #[default]
    Simple,
    /// `j/k/h/l/e/r/g/G` control the conversation surface.
    Vim,
}

/// Result of routing one fullscreen input event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullscreenOutcome {
    Unchanged,
    Changed,
    FocusComposer,
    ForwardToComposer(KeyEvent),
    OpenSearch,
    OpenViewer(SurfaceNodeId),
}

#[derive(Debug, Clone, Copy)]
enum NodeDisplayAction {
    Toggle,
    Collapse,
    Expand,
    ToggleRaw,
}

/// Fullscreen controller over one canonical rendered conversation surface.
/// Refresh after transcript updates or resize; display actions refresh in place.
pub struct FullscreenHost {
    thread_id: String,
    area: Rect,
    surface: ConversationSurface,
    viewport: SurfaceViewport,
    renderer: SurfaceRenderer,
    key_mode: ScrollbackKeyMode,
    pending_click: Option<SurfaceNodeId>,
    last_click: Option<(SurfaceNodeId, Instant)>,
    scrollbar_dragging: bool,
}

impl FullscreenHost {
    pub fn new(conversation: &ConversationState, area: Rect, key_mode: ScrollbackKeyMode) -> Self {
        let surface = render_surface(conversation, area);
        let mut viewport = SurfaceViewport::default();
        viewport.prepare(&surface, area.height);
        Self {
            thread_id: conversation.transcript().thread_id().to_string(),
            area,
            surface,
            viewport,
            renderer: SurfaceRenderer::default(),
            key_mode,
            pending_click: None,
            last_click: None,
            scrollbar_dragging: false,
        }
    }

    pub fn surface(&self) -> &ConversationSurface {
        &self.surface
    }

    pub fn viewport(&self) -> &SurfaceViewport {
        &self.viewport
    }

    pub fn set_key_mode(&mut self, key_mode: ScrollbackKeyMode) {
        self.key_mode = key_mode;
    }

    /// Rebuild after transcript growth or resize, retaining semantic targets.
    pub fn refresh_surface(&mut self, conversation: &ConversationState, area: Rect) {
        let resized = self.area != area;
        let thread_changed = self.thread_id != conversation.transcript().thread_id();
        if thread_changed {
            self.thread_id = conversation.transcript().thread_id().to_string();
        }
        self.area = area;
        self.surface = render_surface(conversation, area);
        self.viewport.prepare(&self.surface, area.height);
        if resized || thread_changed {
            self.cancel_pointer_gesture();
        } else {
            self.pending_click = self
                .pending_click
                .filter(|node| self.surface.node(*node).is_some());
            self.last_click = self
                .last_click
                .filter(|click| self.surface.node(click.0).is_some());
        }
    }

    pub fn render(&self, buffer: &mut Buffer) {
        self.renderer
            .render(self.area, buffer, &self.surface, &self.viewport);
    }

    pub fn handle_key_event(
        &mut self,
        key: KeyEvent,
        conversation: &mut ConversationState,
    ) -> FullscreenOutcome {
        if key.kind == KeyEventKind::Release {
            return FullscreenOutcome::Unchanged;
        }
        self.cancel_pointer_gesture();

        if key.modifiers.is_empty()
            && (matches!(key.code, KeyCode::Tab | KeyCode::Char(' '))
                || (self.key_mode == ScrollbackKeyMode::Vim && key.code == KeyCode::Char('i')))
        {
            return FullscreenOutcome::FocusComposer;
        }

        let outcome = match (key.code, key.modifiers) {
            (KeyCode::Up, KeyModifiers::NONE) => self.move_selection(ScrollDirection::Up),
            (KeyCode::Down, KeyModifiers::NONE) => self.move_selection(ScrollDirection::Down),
            (KeyCode::PageUp, KeyModifiers::NONE) => self.page(ScrollDirection::Up),
            (KeyCode::PageDown, KeyModifiers::NONE) => self.page(ScrollDirection::Down),
            (KeyCode::Home, KeyModifiers::NONE) => self.goto_top(),
            (KeyCode::End, KeyModifiers::NONE) => self.goto_bottom(),
            (KeyCode::Left, KeyModifiers::NONE) => {
                self.apply_selected_display(conversation, NodeDisplayAction::Collapse)
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                self.apply_selected_display(conversation, NodeDisplayAction::Expand)
            }
            (KeyCode::Enter, KeyModifiers::NONE) => self.open_selected(conversation),
            (KeyCode::Char('f'), KeyModifiers::CONTROL) => self.open_selected(conversation),
            (KeyCode::Char('k'), KeyModifiers::CONTROL) => self.handle_scroll_lines(-1),
            (KeyCode::Char('j'), KeyModifiers::CONTROL) => self.handle_scroll_lines(1),
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => self.half_page(ScrollDirection::Up),
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => self.half_page(ScrollDirection::Down),
            _ => FullscreenOutcome::Unchanged,
        };
        if outcome != FullscreenOutcome::Unchanged {
            return outcome;
        }

        if self.key_mode == ScrollbackKeyMode::Vim {
            return match key.code {
                KeyCode::Char('j') if key.modifiers.is_empty() => {
                    self.move_selection(ScrollDirection::Down)
                }
                KeyCode::Char('k') if key.modifiers.is_empty() => {
                    self.move_selection(ScrollDirection::Up)
                }
                KeyCode::Char('h') if key.modifiers.is_empty() => {
                    self.apply_selected_display(conversation, NodeDisplayAction::Collapse)
                }
                KeyCode::Char('l') if key.modifiers.is_empty() => {
                    self.apply_selected_display(conversation, NodeDisplayAction::Expand)
                }
                KeyCode::Char('e') if key.modifiers.is_empty() => {
                    self.apply_selected_display(conversation, NodeDisplayAction::Toggle)
                }
                KeyCode::Char('r') if key.modifiers.is_empty() => {
                    self.apply_selected_display(conversation, NodeDisplayAction::ToggleRaw)
                }
                KeyCode::Char('g') if key.modifiers.is_empty() => self.goto_top(),
                KeyCode::Char('G')
                    if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
                {
                    self.goto_bottom()
                }
                KeyCode::Char('/') if key.modifiers.is_empty() => {
                    if self.surface.nodes().is_empty() {
                        FullscreenOutcome::ForwardToComposer(key)
                    } else {
                        FullscreenOutcome::OpenSearch
                    }
                }
                _ => FullscreenOutcome::Unchanged,
            };
        }

        if let KeyCode::Char(character) = key.code
            && (character.is_ascii_alphabetic() || character == '/')
            && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT)
        {
            return FullscreenOutcome::ForwardToComposer(key);
        }
        FullscreenOutcome::Unchanged
    }

    /// Route pointer selection, hover, wheel scrolling, and scrollbar dragging.
    pub fn handle_mouse_event_at(
        &mut self,
        mouse: MouseEvent,
        now: Instant,
        conversation: &mut ConversationState,
    ) -> FullscreenOutcome {
        match mouse.kind {
            MouseEventKind::Moved => self.update_hover(mouse.column, mouse.row),
            MouseEventKind::ScrollUp => self.handle_scroll_lines(-FALLBACK_WHEEL_ROWS),
            MouseEventKind::ScrollDown => self.handle_scroll_lines(FALLBACK_WHEEL_ROWS),
            MouseEventKind::Down(MouseButton::Left) => {
                if self.scrollbar_hit(mouse.column, mouse.row) {
                    self.scrollbar_dragging = true;
                    self.pending_click = None;
                    self.last_click = None;
                    return changed(self.apply_scrollbar_row(mouse.row));
                }
                self.pending_click = self.node_at_pointer(mouse.column, mouse.row);
                if self.pending_click.is_none() {
                    self.last_click = None;
                }
                FullscreenOutcome::Unchanged
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.scrollbar_dragging {
                    return changed(self.apply_scrollbar_row(mouse.row));
                }
                self.pending_click = None;
                self.last_click = None;
                self.update_hover(mouse.column, mouse.row)
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if self.scrollbar_dragging {
                    self.scrollbar_dragging = false;
                    return changed(self.apply_scrollbar_row(mouse.row));
                }
                let Some(pending) = self.pending_click.take() else {
                    return FullscreenOutcome::Unchanged;
                };
                self.complete_click(pending, now, conversation)
            }
            _ => FullscreenOutcome::Unchanged,
        }
    }

    /// Apply a signed, terminal-normalized line delta. Positive moves down.
    pub fn handle_scroll_lines(&mut self, lines: i32) -> FullscreenOutcome {
        self.cancel_pointer_gesture();
        if lines == 0 {
            return FullscreenOutcome::Unchanged;
        }
        let direction = if lines.is_negative() {
            ScrollDirection::Up
        } else {
            ScrollDirection::Down
        };
        changed(
            self.viewport
                .scroll_rows(&self.surface, direction, lines.unsigned_abs() as usize),
        )
    }

    fn move_selection(&mut self, direction: ScrollDirection) -> FullscreenOutcome {
        changed(self.viewport.move_selection(&self.surface, direction))
    }

    fn page(&mut self, direction: ScrollDirection) -> FullscreenOutcome {
        changed(self.viewport.scroll_page(&self.surface, direction))
    }

    fn half_page(&mut self, direction: ScrollDirection) -> FullscreenOutcome {
        let rows = usize::from((self.viewport.height() / 2).max(1));
        changed(self.viewport.scroll_rows(&self.surface, direction, rows))
    }

    fn goto_top(&mut self) -> FullscreenOutcome {
        let scrolled = self.viewport.scroll_to_top(&self.surface);
        let selected = self.viewport.select_first(&self.surface);
        changed(scrolled || selected)
    }

    fn goto_bottom(&mut self) -> FullscreenOutcome {
        let scrolled = self.viewport.scroll_to_bottom(&self.surface);
        let selected = self.viewport.select_last(&self.surface);
        changed(scrolled || selected)
    }

    fn open_selected(&mut self, conversation: &mut ConversationState) -> FullscreenOutcome {
        let Some(selected) = self.viewport.selected() else {
            return FullscreenOutcome::Unchanged;
        };
        if matches!(selected, SurfaceNodeId::VerbGroup(_)) {
            return self.apply_selected_display(conversation, NodeDisplayAction::Toggle);
        }
        FullscreenOutcome::OpenViewer(selected)
    }

    fn complete_click(
        &mut self,
        node: SurfaceNodeId,
        now: Instant,
        conversation: &mut ConversationState,
    ) -> FullscreenOutcome {
        let selected = self.viewport.select_node(&self.surface, Some(node));
        let double_click = self.last_click.is_some_and(|last| {
            last.0 == node && now.saturating_duration_since(last.1) < MULTI_CLICK_TIMEOUT
        });
        if double_click {
            self.last_click = None;
            let folded = self.apply_node_display(conversation, node, NodeDisplayAction::Toggle);
            return changed(selected || folded);
        }
        self.last_click = Some((node, now));
        changed(selected)
    }

    fn update_hover(&mut self, column: u16, row: u16) -> FullscreenOutcome {
        let before = self.viewport.hovered();
        if self.area_contains(column, row) && !self.scrollbar_column(column) {
            let screen_row = row.saturating_sub(self.area.y);
            self.viewport.hover_screen_row(&self.surface, screen_row);
        } else {
            self.viewport.clear_hover();
        }
        changed(self.viewport.hovered() != before)
    }

    fn apply_selected_display(
        &mut self,
        conversation: &mut ConversationState,
        action: NodeDisplayAction,
    ) -> FullscreenOutcome {
        let Some(selected) = self.viewport.selected() else {
            return FullscreenOutcome::Unchanged;
        };
        changed(self.apply_node_display(conversation, selected, action))
    }

    fn apply_node_display(
        &mut self,
        conversation: &mut ConversationState,
        node_id: SurfaceNodeId,
        action: NodeDisplayAction,
    ) -> bool {
        let Some(node) = self.surface.node(node_id) else {
            return false;
        };
        let previous_row = node.rows().start;
        let before = node.display_mode();
        let turn_id = node.turn_id().to_string();
        let changed = match node_id {
            SurfaceNodeId::Entry(entry_id) => {
                let action = match action {
                    NodeDisplayAction::Toggle => EntryDisplayAction::ToggleFold,
                    NodeDisplayAction::Collapse => EntryDisplayAction::Collapse,
                    NodeDisplayAction::Expand => EntryDisplayAction::Expand,
                    NodeDisplayAction::ToggleRaw => EntryDisplayAction::ToggleRaw,
                };
                conversation.apply_entry_display_action(entry_id, action)
            }
            SurfaceNodeId::VerbGroup(anchor) => {
                let action = match action {
                    NodeDisplayAction::Toggle => VerbGroupDisplayAction::Toggle,
                    NodeDisplayAction::Collapse => VerbGroupDisplayAction::Collapse,
                    NodeDisplayAction::Expand => VerbGroupDisplayAction::Expand,
                    NodeDisplayAction::ToggleRaw => return false,
                };
                conversation
                    .apply_verb_group_display_action(&turn_id, anchor, action)
                    .is_some_and(|after| after != before)
            }
        };
        if changed {
            self.refresh_after_display_change(conversation, node_id, previous_row);
        }
        changed
    }

    fn refresh_after_display_change(
        &mut self,
        conversation: &ConversationState,
        selected: SurfaceNodeId,
        previous_row: usize,
    ) {
        self.refresh_surface(conversation, self.area);
        if self.viewport.selected().is_some() {
            return;
        }
        let fallback = self
            .surface
            .node(selected)
            .map(super::surface::SurfaceNode::id)
            .or_else(|| {
                self.surface
                    .nodes()
                    .iter()
                    .find(|node| node.rows().end > previous_row)
                    .or_else(|| self.surface.nodes().last())
                    .map(super::surface::SurfaceNode::id)
            });
        self.viewport.select_node(&self.surface, fallback);
    }

    fn node_at_pointer(&self, column: u16, row: u16) -> Option<SurfaceNodeId> {
        if !self.area_contains(column, row) || self.scrollbar_column(column) {
            return None;
        }
        let virtual_row = self
            .viewport
            .top()
            .saturating_add(usize::from(row.saturating_sub(self.area.y)));
        self.surface
            .node_at_row(virtual_row)
            .map(super::surface::SurfaceNode::id)
    }

    fn area_contains(&self, column: u16, row: u16) -> bool {
        column >= self.area.x
            && column < self.area.right()
            && row >= self.area.y
            && row < self.area.bottom()
    }

    fn cancel_pointer_gesture(&mut self) {
        self.pending_click = None;
        self.last_click = None;
        self.scrollbar_dragging = false;
    }

    fn scrollbar_hit(&self, column: u16, row: u16) -> bool {
        self.area_contains(column, row)
            && self.scrollbar_column(column)
            && self.surface.row_count() > usize::from(self.area.height)
    }

    fn scrollbar_column(&self, column: u16) -> bool {
        self.area.width > 1 && column == self.area.right().saturating_sub(1)
    }

    fn apply_scrollbar_row(&mut self, row: u16) -> bool {
        let height = usize::from(self.area.height);
        let total = self.surface.row_count();
        let maximum = total.saturating_sub(height);
        let click = usize::from(
            row.saturating_sub(self.area.y)
                .min(self.area.height.saturating_sub(1)),
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
        self.viewport.scroll_to_row(&self.surface, target)
    }
}

fn render_surface(conversation: &ConversationState, area: Rect) -> ConversationSurface {
    ConversationSurface::render(
        conversation,
        EntryRenderOptions::new(SurfaceRenderer::content_width(area)),
    )
}

fn changed(value: bool) -> FullscreenOutcome {
    if value {
        FullscreenOutcome::Changed
    } else {
        FullscreenOutcome::Unchanged
    }
}

#[cfg(test)]
#[path = "fullscreen_tests.rs"]
mod tests;
