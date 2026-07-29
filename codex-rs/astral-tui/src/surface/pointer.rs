use std::time::Duration;
use std::time::Instant;

use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::layout::Position;
use ratatui::layout::Rect;

use crate::composer::ComposerMouseAction;
use crate::view::CompletionMenuFrame;
use crate::view::QueuePaneFrame;
use crate::view::QueuePaneHover;
use crate::view::prompt_cursor_at;
use crate::view::prompt_drag_cursor_at;

use super::SurfaceState;

/// Hit regions from the last rendered frame that decide which pane owns a
/// pointer event.
#[derive(Debug, Default)]
pub(super) struct SurfacePointerState {
    scrollback: Rect,
    prompt: Rect,
    queue: QueuePaneFrame,
    queue_hovered: Option<QueuePaneHover>,
    completion: Option<CompletionMenuFrame>,
    completion_hovered: Option<usize>,
    completion_scrollbar_dragging: bool,
    pending_prompt_drag: Option<MouseEvent>,
    last_prompt_drag_scroll: Option<Instant>,
    prompt_drag_scroll_steps: u32,
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

    fn observe_queue(&mut self, frame: QueuePaneFrame) {
        if self
            .queue_hovered
            .is_some_and(|hover| !frame.contains_id(hover.id))
        {
            self.queue_hovered = None;
        }
        self.queue = frame;
    }

    fn queue_contains(&self, mouse: MouseEvent) -> bool {
        self.queue.contains(mouse.column, mouse.row)
    }

    fn queue_hit(&self, mouse: MouseEvent) -> Option<QueuePaneHover> {
        self.queue.hit(mouse.column, mouse.row)
    }

    fn update_queue_hover(&mut self, mouse: MouseEvent) -> bool {
        let hovered = self.queue.hit(mouse.column, mouse.row);
        if hovered == self.queue_hovered {
            return false;
        }
        self.queue_hovered = hovered;
        true
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

    pub(crate) fn observe_queue_frame(&mut self, frame: QueuePaneFrame) {
        self.pointer_areas.observe_queue(frame);
    }

    pub(crate) fn queue_contains(&self, mouse: MouseEvent) -> bool {
        self.pointer_areas.queue_contains(mouse)
    }

    pub(crate) fn queue_hit(&self, mouse: MouseEvent) -> Option<QueuePaneHover> {
        self.pointer_areas.queue_hit(mouse)
    }

    pub(crate) fn queue_hovered(&self) -> Option<QueuePaneHover> {
        self.pointer_areas.queue_hovered
    }

    pub(crate) fn update_queue_hover(&mut self, mouse: MouseEvent) -> bool {
        self.pointer_areas.update_queue_hover(mouse)
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
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => self.clear_prompt_drag_scroll(),
            MouseEventKind::Drag(MouseButton::Left) => {
                if !self.composer.mouse_selection_active() {
                    return ComposerMouseAction::Nothing;
                }
                if self.prompt_drag_outside_vertically(mouse) {
                    self.pointer_areas.pending_prompt_drag = Some(mouse);
                    let interval =
                        prompt_drag_interval(self.pointer_areas.prompt_drag_scroll_steps);
                    if self
                        .pointer_areas
                        .last_prompt_drag_scroll
                        .is_some_and(|last| {
                            now.checked_duration_since(last)
                                .is_some_and(|elapsed| elapsed < interval)
                        })
                    {
                        return ComposerMouseAction::Nothing;
                    }
                    self.pointer_areas.last_prompt_drag_scroll = Some(now);
                    self.pointer_areas.prompt_drag_scroll_steps = self
                        .pointer_areas
                        .prompt_drag_scroll_steps
                        .saturating_add(1);
                } else {
                    self.clear_prompt_drag_scroll();
                }
            }
            MouseEventKind::Up(MouseButton::Left) => self.clear_prompt_drag_scroll(),
            MouseEventKind::Down(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Up(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Drag(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Moved
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight => {}
        }
        let position = match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => prompt_cursor_at(
                self.composer.text(),
                self.composer.cursor(),
                self.pointer_areas.prompt,
                Position::new(mouse.column, mouse.row),
            ),
            MouseEventKind::Drag(MouseButton::Left) => prompt_drag_cursor_at(
                self.composer.text(),
                self.composer.cursor(),
                self.pointer_areas.prompt,
                Position::new(mouse.column, mouse.row),
            ),
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

    pub(crate) fn composer_drag_deadline(&self) -> Option<Instant> {
        if !self.composer.mouse_selection_active() {
            return None;
        }
        self.pointer_areas.pending_prompt_drag?;
        self.pointer_areas
            .last_prompt_drag_scroll
            .map(|last| last + prompt_drag_interval(self.pointer_areas.prompt_drag_scroll_steps))
    }

    pub(crate) fn tick_composer_drag(&mut self, now: Instant) -> bool {
        let Some(event) = self.pointer_areas.pending_prompt_drag else {
            return false;
        };
        if self
            .composer_drag_deadline()
            .is_some_and(|deadline| now < deadline)
        {
            return false;
        }
        self.handle_composer_mouse(event, now) != ComposerMouseAction::Nothing
    }

    fn prompt_drag_outside_vertically(&self, mouse: MouseEvent) -> bool {
        mouse.row <= self.pointer_areas.prompt.y
            || mouse.row >= self.pointer_areas.prompt.bottom().saturating_sub(1)
    }

    fn clear_prompt_drag_scroll(&mut self) {
        self.pointer_areas.pending_prompt_drag = None;
        self.pointer_areas.last_prompt_drag_scroll = None;
        self.pointer_areas.prompt_drag_scroll_steps = 0;
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

fn prompt_drag_interval(step: u32) -> Duration {
    const RAMP: [Duration; 3] = [
        Duration::from_millis(80),
        Duration::from_millis(60),
        Duration::from_millis(40),
    ];
    RAMP[RAMP.len().min(step as usize + 1) - 1]
}
