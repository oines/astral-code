use std::time::Instant;

use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::layout::Position;
use ratatui::layout::Rect;

use crate::composer::ComposerMouseAction;
use crate::view::CompletionMenuFrame;
use crate::view::prompt_cursor_at;

use super::SurfaceState;

/// Hit regions from the last rendered frame that decide which pane owns a
/// pointer event.
#[derive(Debug, Default)]
pub(super) struct SurfacePointerState {
    scrollback: Rect,
    prompt: Rect,
    completion: Option<CompletionMenuFrame>,
    completion_hovered: Option<usize>,
    completion_scrollbar_dragging: bool,
}

impl SurfacePointerState {
    fn observe(&mut self, scrollback: Rect, prompt: Rect) {
        self.scrollback = scrollback;
        self.prompt = prompt;
    }

    fn scrollback_contains(&self, mouse: MouseEvent) -> bool {
        self.scrollback.contains((mouse.column, mouse.row).into())
    }

    fn scrollback_rows(&self) -> u16 {
        self.scrollback.height
    }

    fn prompt_contains(&self, mouse: MouseEvent) -> bool {
        self.prompt.contains((mouse.column, mouse.row).into())
    }

    fn observe_completion(&mut self, frame: CompletionMenuFrame) {
        if self
            .completion_hovered
            .is_some_and(|item| !frame.contains_item(item))
        {
            self.completion_hovered = None;
        }
        self.completion = Some(frame);
    }

    fn clear_completion(&mut self) {
        self.completion = None;
        self.completion_hovered = None;
        self.completion_scrollbar_dragging = false;
    }

    fn completion_contains(&self, mouse: MouseEvent) -> bool {
        self.completion
            .as_ref()
            .is_some_and(|frame| frame.contains(mouse.column, mouse.row))
    }

    fn completion_row_at(&self, mouse: MouseEvent) -> Option<usize> {
        self.completion.as_ref()?.row_at(mouse.column, mouse.row)
    }

    fn completion_scrollbar_target(&self, mouse: MouseEvent, total_items: usize) -> Option<usize> {
        self.completion
            .as_ref()?
            .scrollbar_target(mouse.column, mouse.row, total_items)
    }

    fn completion_visible_rows(&self) -> Option<usize> {
        self.completion
            .as_ref()
            .map(CompletionMenuFrame::visible_rows)
    }

    fn update_completion_hover(&mut self, mouse: MouseEvent) -> bool {
        let hovered = self.completion_row_at(mouse);
        if hovered == self.completion_hovered {
            return false;
        }
        self.completion_hovered = hovered;
        true
    }
}

impl SurfaceState {
    pub(crate) fn observe_pointer_areas(&mut self, scrollback: Rect, prompt: Rect) {
        self.pointer_areas.observe(scrollback, prompt);
    }

    pub(crate) fn scrollback_contains(&self, mouse: MouseEvent) -> bool {
        self.pointer_areas.scrollback_contains(mouse)
    }

    pub(crate) fn scrollback_rows(&self) -> u16 {
        self.pointer_areas.scrollback_rows()
    }

    pub(crate) fn prompt_contains(&self, mouse: MouseEvent) -> bool {
        self.pointer_areas.prompt_contains(mouse)
    }

    pub(crate) fn composer_mouse_active(&self) -> bool {
        self.composer.mouse_selection_active()
    }

    pub(crate) fn composer_mouse_drag_active(&self) -> bool {
        self.composer.mouse_drag_active()
    }

    pub(crate) fn handle_composer_mouse(
        &mut self,
        mut mouse: MouseEvent,
        now: Instant,
    ) -> ComposerMouseAction {
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            && self.composer.mouse_drag_active()
        {
            mouse.kind = MouseEventKind::Drag(MouseButton::Left);
        }
        let position = match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left) => {
                prompt_cursor_at(
                    self.composer.text(),
                    self.composer.cursor(),
                    self.pointer_areas.prompt,
                    Position::new(mouse.column, mouse.row),
                )
            }
            MouseEventKind::Up(MouseButton::Left)
            | MouseEventKind::Down(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Up(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Drag(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Moved
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight => None,
        };
        let action = self.composer.handle_mouse(mouse, position, now);
        if action != ComposerMouseAction::Nothing {
            self.refresh_composer_completions();
        }
        action
    }

    pub(crate) fn observe_completion_menu(&mut self, frame: CompletionMenuFrame) {
        self.pointer_areas.observe_completion(frame);
    }

    pub(crate) fn clear_completion_menu(&mut self) {
        self.pointer_areas.clear_completion();
    }

    pub(crate) fn completion_hovered(&self) -> Option<usize> {
        self.pointer_areas.completion_hovered
    }

    pub(crate) fn completion_contains(&self, mouse: MouseEvent) -> bool {
        self.pointer_areas.completion_contains(mouse)
    }

    pub(crate) fn completion_row_at(&self, mouse: MouseEvent) -> Option<usize> {
        self.pointer_areas.completion_row_at(mouse)
    }

    pub(crate) fn completion_scrollbar_target(
        &self,
        mouse: MouseEvent,
        total_items: usize,
    ) -> Option<usize> {
        self.pointer_areas
            .completion_scrollbar_target(mouse, total_items)
    }

    pub(crate) fn completion_visible_rows(&self) -> Option<usize> {
        self.pointer_areas.completion_visible_rows()
    }

    pub(crate) fn update_completion_hover(&mut self, mouse: MouseEvent) -> bool {
        self.pointer_areas.update_completion_hover(mouse)
    }

    pub(crate) fn set_completion_scrollbar_dragging(&mut self, dragging: bool) {
        self.pointer_areas.completion_scrollbar_dragging = dragging;
    }

    pub(crate) fn completion_scrollbar_dragging(&self) -> bool {
        self.pointer_areas.completion_scrollbar_dragging
    }
}
