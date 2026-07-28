//! Shared state for Astral modal windows.

use crossterm::event::MouseButton;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModalRowHit {
    pub(crate) id: usize,
    pub(crate) area: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModalPointerAction {
    Ignored,
    Redraw,
    Close,
    Hover(Option<usize>),
    Activate(usize),
    Scroll(isize),
}

/// Last-frame modal hit map shared by Astral's pickers and information panels.
///
/// Like Grok Build's `ModalWindowState`, pointer input is resolved against the
/// exact geometry that was rendered rather than recomputing popup coordinates
/// in the input path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ModalPointerState {
    popup: Option<Rect>,
    close_button: Option<Rect>,
    rows: Vec<ModalRowHit>,
    close_hovered: bool,
    hovered_row: Option<usize>,
}

impl ModalPointerState {
    pub(crate) fn observe_frame(
        &mut self,
        popup: Rect,
        close_button: Rect,
        rows: Vec<ModalRowHit>,
    ) {
        self.popup = Some(popup);
        self.close_button = Some(close_button);
        self.rows = rows;
        if self
            .hovered_row
            .is_some_and(|hovered| !self.rows.iter().any(|row| row.id == hovered))
        {
            self.hovered_row = None;
        }
    }

    pub(crate) fn close_hovered(&self) -> bool {
        self.close_hovered
    }

    pub(crate) fn handle_mouse(&mut self, mouse: MouseEvent) -> ModalPointerAction {
        let position = (mouse.column, mouse.row).into();
        let on_close = self
            .close_button
            .is_some_and(|area| area.contains(position));
        let on_row = self
            .rows
            .iter()
            .find(|row| row.area.contains(position))
            .map(|row| row.id);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if on_close || self.popup.is_some_and(|area| !area.contains(position)) {
                    ModalPointerAction::Close
                } else if let Some(row) = on_row {
                    ModalPointerAction::Activate(row)
                } else {
                    ModalPointerAction::Redraw
                }
            }
            MouseEventKind::Moved => {
                let changed = self.close_hovered != on_close || self.hovered_row != on_row;
                self.close_hovered = on_close;
                self.hovered_row = on_row;
                if changed {
                    ModalPointerAction::Hover(on_row)
                } else {
                    ModalPointerAction::Ignored
                }
            }
            MouseEventKind::ScrollUp => ModalPointerAction::Scroll(-3),
            MouseEventKind::ScrollDown => ModalPointerAction::Scroll(3),
            MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight
            | MouseEventKind::Up(_)
            | MouseEventKind::Drag(_)
            | MouseEventKind::Down(MouseButton::Right | MouseButton::Middle) => {
                ModalPointerAction::Ignored
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModalRow {
    pub(crate) label: String,
    pub(crate) value: String,
}

impl ModalRow {
    pub(crate) fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModalState {
    pub(crate) title: String,
    pub(crate) rows: Vec<ModalRow>,
    pub(crate) scroll_offset: usize,
    pub(crate) pointer: ModalPointerState,
}

impl ModalState {
    pub(crate) fn info(title: impl Into<String>, rows: Vec<ModalRow>) -> Self {
        Self {
            title: title.into(),
            rows,
            scroll_offset: 0,
            pointer: ModalPointerState::default(),
        }
    }

    pub(crate) fn scroll_by(&mut self, delta: isize) {
        self.scroll_offset = self
            .scroll_offset
            .saturating_add_signed(delta)
            .min(self.rows.len().saturating_sub(1));
    }

    pub(crate) fn scroll_to_start(&mut self) {
        self.scroll_offset = 0;
    }

    pub(crate) fn scroll_to_end(&mut self) {
        self.scroll_offset = self.rows.len().saturating_sub(1);
    }
}
