use crossterm::event::MouseEvent;
use ratatui::layout::Rect;

use super::SurfaceState;

/// Hit regions from the last rendered frame that decide which pane owns a
/// pointer event.
#[derive(Debug, Default)]
pub(super) struct SurfacePointerState {
    scrollback: Rect,
    prompt: Rect,
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
}
