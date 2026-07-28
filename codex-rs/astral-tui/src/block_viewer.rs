// Derived from Grok Build's block-viewer modal and pointer behavior at commit
// 47348d13ec4508dcfe440e34c6d511bb02998fb2 (Apache-2.0).
// Modified to retain Astral's stable transcript entry id and render the current
// provider-neutral PresentationBlock instead of copying runtime payloads.

use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockViewerMouseAction {
    Ignored,
    Redraw,
    Close,
}

/// Focus and viewport state for one transcript block viewer.
///
/// The block itself deliberately remains in `ConversationState`; keeping only
/// its stable entry id here lets live updates and resume use the same canonical
/// projection while the viewer is open.
#[derive(Debug)]
pub(crate) struct BlockViewerState {
    entry_id: String,
    scroll_offset: usize,
    max_scroll_offset: usize,
    page_size: usize,
    popup_area: Option<Rect>,
    close_button: Option<Rect>,
    close_hovered: bool,
}

impl BlockViewerState {
    pub(crate) fn new(entry_id: String) -> Self {
        Self {
            entry_id,
            scroll_offset: 0,
            max_scroll_offset: 0,
            page_size: 1,
            popup_area: None,
            close_button: None,
            close_hovered: false,
        }
    }

    pub(crate) fn entry_id(&self) -> &str {
        &self.entry_id
    }

    pub(crate) fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub(crate) fn close_hovered(&self) -> bool {
        self.close_hovered
    }

    pub(crate) fn observe_frame(
        &mut self,
        popup_area: Rect,
        content_area: Rect,
        close_button: Rect,
        total_lines: usize,
    ) {
        self.popup_area = Some(popup_area);
        self.close_button = Some(close_button);
        self.page_size = usize::from(content_area.height).max(1);
        self.max_scroll_offset = total_lines.saturating_sub(self.page_size);
        self.scroll_offset = self.scroll_offset.min(self.max_scroll_offset);
    }

    pub(crate) fn scroll_by(&mut self, lines: isize) -> bool {
        let next = self
            .scroll_offset
            .saturating_add_signed(lines)
            .min(self.max_scroll_offset);
        if next == self.scroll_offset {
            return false;
        }
        self.scroll_offset = next;
        true
    }

    pub(crate) fn scroll_page(&mut self, pages: isize) -> bool {
        let lines = isize::try_from(self.page_size.saturating_sub(1).max(1))
            .unwrap_or(isize::MAX)
            .saturating_mul(pages);
        self.scroll_by(lines)
    }

    pub(crate) fn scroll_to_start(&mut self) -> bool {
        if self.scroll_offset == 0 {
            return false;
        }
        self.scroll_offset = 0;
        true
    }

    pub(crate) fn scroll_to_end(&mut self) -> bool {
        if self.scroll_offset == self.max_scroll_offset {
            return false;
        }
        self.scroll_offset = self.max_scroll_offset;
        true
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
                    BlockViewerMouseAction::Close
                } else {
                    BlockViewerMouseAction::Ignored
                }
            }
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
                self.scroll_by(-3);
                BlockViewerMouseAction::Redraw
            }
            MouseEventKind::ScrollDown => {
                self.scroll_by(3);
                BlockViewerMouseAction::Redraw
            }
            MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight
            | MouseEventKind::Up(_)
            | MouseEventKind::Drag(_)
            | MouseEventKind::Down(MouseButton::Right | MouseButton::Middle) => {
                BlockViewerMouseAction::Ignored
            }
        }
    }
}

#[cfg(test)]
#[path = "block_viewer_tests.rs"]
mod tests;
