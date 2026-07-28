//! Pointer interaction for MCP elicitation forms.
//!
//! List hit-testing, hover, and repeated-click activation follow Grok Build's
//! question view at commit 47348d13ec4508dcfe440e34c6d511bb02998fb2
//! (Apache-2.0). Astral retains Codex's typed elicitation response contract and
//! field validation; this module only owns pointer interaction.

use std::time::Duration;
use std::time::Instant;

use codex_app_server_protocol::McpElicitationSchema;
use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::layout::Rect;

use super::McpFormEvent;
use super::McpFormHit;
use super::McpFormState;

const MULTI_CLICK_TIMEOUT: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct McpFormPointerState {
    hovered: Option<McpFormHit>,
    hit_rows: Vec<(McpFormHit, Rect)>,
    last_click: Option<(Instant, McpFormHit)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PointerAction {
    Consume,
    Select(McpFormHit),
    Activate(McpFormHit),
    Scroll { next: bool },
}

impl McpFormPointerState {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    fn observe_rows(&mut self, hit_rows: Vec<(McpFormHit, Rect)>) {
        self.hit_rows = hit_rows;
        if self
            .hovered
            .is_some_and(|hovered| !self.hit_rows.iter().any(|(hit, _)| *hit == hovered))
        {
            self.hovered = None;
        }
        if self
            .last_click
            .is_some_and(|(_, clicked)| !self.hit_rows.iter().any(|(hit, _)| *hit == clicked))
        {
            self.last_click = None;
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, now: Instant) -> PointerAction {
        let hit = self.hit_test(mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::Moved => {
                self.hovered = hit;
                PointerAction::Consume
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(hit) = hit else {
                    self.last_click = None;
                    return PointerAction::Consume;
                };
                self.hovered = Some(hit);
                if hit == McpFormHit::Editor {
                    self.last_click = None;
                    return PointerAction::Activate(hit);
                }
                let double_click = self.last_click.is_some_and(|(last, previous)| {
                    previous == hit && now.duration_since(last) < MULTI_CLICK_TIMEOUT
                });
                if double_click {
                    self.last_click = None;
                    PointerAction::Activate(hit)
                } else {
                    self.last_click = Some((now, hit));
                    PointerAction::Select(hit)
                }
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
                if matches!(hit, Some(McpFormHit::Choice(_))) =>
            {
                self.last_click = None;
                PointerAction::Scroll {
                    next: matches!(mouse.kind, MouseEventKind::ScrollDown),
                }
            }
            MouseEventKind::Drag(MouseButton::Left)
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight => {
                self.last_click = None;
                PointerAction::Consume
            }
            MouseEventKind::Down(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Up(MouseButton::Left | MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Drag(MouseButton::Right | MouseButton::Middle) => {
                PointerAction::Consume
            }
        }
    }

    fn hit_test(&self, column: u16, row: u16) -> Option<McpFormHit> {
        self.hit_rows
            .iter()
            .find(|(_, area)| area.contains((column, row).into()))
            .map(|(hit, _)| *hit)
    }
}

impl McpFormState {
    pub(crate) fn hovered(&self) -> Option<McpFormHit> {
        self.pointer.hovered
    }

    pub(crate) fn observe_rows(&mut self, hit_rows: Vec<(McpFormHit, Rect)>) {
        self.pointer.observe_rows(hit_rows);
    }

    pub(crate) fn handle_mouse(
        &mut self,
        schema: &McpElicitationSchema,
        mouse: MouseEvent,
    ) -> McpFormEvent {
        self.handle_mouse_at(schema, mouse, Instant::now())
    }

    pub(super) fn handle_mouse_at(
        &mut self,
        schema: &McpElicitationSchema,
        mouse: MouseEvent,
        now: Instant,
    ) -> McpFormEvent {
        self.sync(schema);
        match self.pointer.handle_mouse(mouse, now) {
            PointerAction::Consume | PointerAction::Select(McpFormHit::Editor) => {
                McpFormEvent::Redraw
            }
            PointerAction::Scroll { next } => {
                self.move_choice(next);
                McpFormEvent::Redraw
            }
            PointerAction::Select(McpFormHit::Choice(index)) => {
                self.toggle_choice_at(index);
                McpFormEvent::Redraw
            }
            PointerAction::Activate(McpFormHit::Editor) => McpFormEvent::Redraw,
            PointerAction::Activate(McpFormHit::Choice(index)) => {
                self.select_choice_at(index);
                self.advance_or_submit()
            }
        }
    }
}
