//! Astral chrome-theme picker with live preview and cancel restoration.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::MouseEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::modal::ModalPointerAction;
use crate::modal::ModalPointerState;
use crate::modal::ModalRowHit;
use crate::view::AstralTheme;
use crate::view::AstralThemeId;
use crate::view::ModalHeight;
use crate::view::modal_choice_style;
use crate::view::render_modal_close_button;
use crate::view::render_modal_frame_with_geometry;

#[derive(Debug)]
pub(crate) struct ThemePickerState {
    original: AstralThemeId,
    selected: usize,
    pointer: ModalPointerState,
}

impl ThemePickerState {
    pub(crate) fn new(current: AstralThemeId) -> Self {
        let selected = AstralThemeId::ALL
            .iter()
            .position(|theme| *theme == current)
            .unwrap_or_default();
        Self {
            original: current,
            selected,
            pointer: ModalPointerState::default(),
        }
    }

    pub(crate) fn original(&self) -> AstralThemeId {
        self.original
    }

    fn selection(&self) -> AstralThemeId {
        AstralThemeId::ALL[self.selected.min(AstralThemeId::ALL.len() - 1)]
    }

    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn move_down(&mut self) {
        self.selected = (self.selected + 1).min(AstralThemeId::ALL.len() - 1);
    }

    fn move_by(&mut self, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(AstralThemeId::ALL.len() - 1);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThemePickerInput {
    None,
    Redraw,
    Preview(AstralThemeId),
    Select(AstralThemeId),
    Cancel,
}

pub(crate) fn handle_key(state: &mut ThemePickerState, key: KeyEvent) -> ThemePickerInput {
    if key.kind == KeyEventKind::Release {
        return ThemePickerInput::None;
    }
    match key.code {
        KeyCode::Esc => ThemePickerInput::Cancel,
        KeyCode::Up | KeyCode::Char('k') => {
            state.move_up();
            ThemePickerInput::Preview(state.selection())
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.move_down();
            ThemePickerInput::Preview(state.selection())
        }
        KeyCode::Enter => ThemePickerInput::Select(state.selection()),
        _ => ThemePickerInput::None,
    }
}

pub(crate) fn handle_mouse(state: &mut ThemePickerState, mouse: MouseEvent) -> ThemePickerInput {
    match state.pointer.handle_mouse(mouse) {
        ModalPointerAction::Ignored => ThemePickerInput::None,
        ModalPointerAction::Redraw | ModalPointerAction::Hover(None) => ThemePickerInput::Redraw,
        ModalPointerAction::Close => ThemePickerInput::Cancel,
        ModalPointerAction::Hover(Some(index)) => {
            state.selected = index.min(AstralThemeId::ALL.len() - 1);
            ThemePickerInput::Preview(state.selection())
        }
        ModalPointerAction::Activate(index) => {
            state.selected = index.min(AstralThemeId::ALL.len() - 1);
            ThemePickerInput::Select(state.selection())
        }
        ModalPointerAction::Scroll(delta) => {
            state.move_by(delta);
            ThemePickerInput::Preview(state.selection())
        }
    }
}

pub(crate) fn render_picker(
    state: &mut ThemePickerState,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    let Some(frame) = render_modal_frame_with_geometry(
        area,
        buffer,
        theme,
        "Choose Astral theme",
        "↑/↓ preview · Enter select · Esc cancel",
        ModalHeight::MinimumContent(8),
    ) else {
        return;
    };
    render_modal_close_button(
        buffer,
        frame.close_button,
        theme,
        state.pointer.close_hovered(),
    );
    let content = frame.content;
    let mut row_hits = Vec::new();
    for (index, option) in AstralThemeId::ALL.iter().enumerate() {
        let y = content.y + u16::try_from(index * 2).unwrap_or(u16::MAX);
        if y >= content.bottom() {
            break;
        }
        row_hits.push(ModalRowHit {
            id: index,
            area: Rect::new(
                content.x,
                y,
                content.width,
                content.bottom().saturating_sub(y).min(2),
            ),
        });
        let selected = index == state.selected;
        let current = *option == state.original;
        let marker = if selected { "❯ " } else { "  " };
        let suffix = if current { " (current)" } else { "" };
        let row_style = modal_choice_style(theme, selected);
        buffer.set_style(Rect::new(content.x, y, content.width, 1), row_style);
        buffer.set_stringn(
            content.x,
            y,
            format!("{marker}{}{suffix}", option.label()),
            usize::from(content.width),
            row_style,
        );
        if y + 1 < content.bottom() {
            buffer.set_stringn(
                content.x + 4,
                y + 1,
                option.description(),
                usize::from(content.width.saturating_sub(4)),
                Style::default().fg(theme.gray).bg(theme.bg_base),
            );
        }
    }
    state
        .pointer
        .observe_frame(frame.popup, frame.close_button, row_hits);
}

#[cfg(test)]
#[path = "theme_picker_tests.rs"]
mod tests;
