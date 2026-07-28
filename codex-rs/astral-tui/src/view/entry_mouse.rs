// Derived from Grok Build's scrollback click-to-fold behavior at commit
// 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Modified for Astral's line-based transcript viewport and stable local ids.

use std::time::Duration;
use std::time::Instant;

use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::layout::Rect;

use super::ScrollbackViewport;
use super::transcript::TranscriptLayout;
use super::transcript::TranscriptSection;

const MULTI_CLICK_TIMEOUT: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EntryMouseAction {
    Ignored,
    Select(String),
    Toggle(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EntryMouseFrame {
    area: Rect,
    viewport: ScrollbackViewport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingClick {
    item_id: String,
}

#[derive(Debug, Clone)]
struct CompletedClick {
    at: Instant,
    item_id: String,
}

/// Mouse hit-testing for foldable transcript entries.
///
/// Text selection and entry folding deliberately remain separate state
/// machines. A drag cancels the pending entry click, while a click selects the
/// entry and a second click on the same entry within the timeout toggles it.
#[derive(Debug, Default)]
pub(crate) struct EntryMouseState {
    frame: Option<EntryMouseFrame>,
    sections: Vec<TranscriptSection>,
    pending: Option<PendingClick>,
    last_click: Option<CompletedClick>,
}

impl EntryMouseState {
    pub(crate) fn observe(
        &mut self,
        layout: &TranscriptLayout,
        viewport: ScrollbackViewport,
        area: Rect,
    ) {
        self.frame = Some(EntryMouseFrame { area, viewport });
        self.sections.clone_from(&layout.sections);
    }

    pub(crate) fn clear_frame(&mut self) {
        self.frame = None;
        self.sections.clear();
        self.pending = None;
    }

    pub(crate) fn handle_mouse(&mut self, mouse: MouseEvent) -> EntryMouseAction {
        self.handle_mouse_at(mouse, Instant::now())
    }

    fn handle_mouse_at(&mut self, mouse: MouseEvent, now: Instant) -> EntryMouseAction {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.pending = self
                    .hit_test(mouse.column, mouse.row)
                    .map(|item_id| PendingClick { item_id });
                if self.pending.is_none() {
                    self.last_click = None;
                }
                EntryMouseAction::Ignored
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                self.pending = None;
                EntryMouseAction::Ignored
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let Some(pending) = self.pending.take() else {
                    return EntryMouseAction::Ignored;
                };
                let Some(item_id) = self.hit_test(mouse.column, mouse.row) else {
                    self.last_click = None;
                    return EntryMouseAction::Ignored;
                };
                if item_id != pending.item_id {
                    self.last_click = None;
                    return EntryMouseAction::Ignored;
                }
                if self.last_click.as_ref().is_some_and(|last| {
                    last.item_id == item_id
                        && now.saturating_duration_since(last.at) < MULTI_CLICK_TIMEOUT
                }) {
                    self.last_click = None;
                    EntryMouseAction::Toggle(item_id)
                } else {
                    self.last_click = Some(CompletedClick {
                        at: now,
                        item_id: item_id.clone(),
                    });
                    EntryMouseAction::Select(item_id)
                }
            }
            MouseEventKind::ScrollDown
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight => {
                self.pending = None;
                self.last_click = None;
                EntryMouseAction::Ignored
            }
            MouseEventKind::Moved
            | MouseEventKind::Down(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Up(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Drag(MouseButton::Right | MouseButton::Middle) => {
                EntryMouseAction::Ignored
            }
        }
    }

    fn hit_test(&self, column: u16, row: u16) -> Option<String> {
        let frame = self.frame?;
        if !frame.area.contains((column, row).into()) {
            return None;
        }
        let line = frame
            .viewport
            .first_visible_line
            .saturating_add(usize::from(row.saturating_sub(frame.area.y)));
        self.sections
            .iter()
            .find(|section| section.lines.contains(&line))
            .map(|section| section.item_id.clone())
    }
}

#[cfg(test)]
#[path = "entry_mouse_tests.rs"]
mod tests;
