//! Astral chrome-theme picker with live preview and cancel restoration.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::view::AstralTheme;
use crate::view::AstralThemeId;
use crate::view::ModalHeight;
use crate::view::modal_choice_style;
use crate::view::render_modal_frame;

#[derive(Debug)]
pub(crate) struct ThemePickerState {
    original: AstralThemeId,
    selected: usize,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThemePickerInput {
    None,
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

pub(crate) fn render_picker(
    state: &ThemePickerState,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    let Some(content) = render_modal_frame(
        area,
        buffer,
        theme,
        "Choose Astral theme",
        "↑/↓ preview · Enter select · Esc cancel",
        ModalHeight::MinimumContent(8),
    ) else {
        return;
    };
    for (index, option) in AstralThemeId::ALL.iter().enumerate() {
        let y = content.y + u16::try_from(index * 2).unwrap_or(u16::MAX);
        if y >= content.bottom() {
            break;
        }
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
}

#[cfg(test)]
#[path = "theme_picker_tests.rs"]
mod tests;
