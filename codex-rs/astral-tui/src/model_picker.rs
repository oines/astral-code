//! Grok-style model and reasoning-effort argument picker.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;

use crate::modal::ModalPointerAction;
use crate::modal::ModalPointerState;
use crate::modal::ModalRowHit;
use crate::model_command::ModelCatalog;
use crate::model_command::ModelSuggestion;
use crate::view::AstralTheme;
use crate::view::ModalSizing;
use crate::view::modal_choice_style;
use crate::view::render_modal_close_button;
use crate::view::render_modal_frame_with_sizing;

const MODEL_PREFIX: &str = "/model ";

#[derive(Debug)]
enum ModelPickerPhase {
    Model,
    Effort { model_args: String },
}

#[derive(Debug)]
pub(crate) struct ModelPickerState {
    catalog: ModelCatalog,
    phase: ModelPickerPhase,
    query: String,
    suggestions: Vec<ModelSuggestion>,
    selected: usize,
    scroll_offset: usize,
    pointer: ModalPointerState,
}

impl ModelPickerState {
    pub(crate) fn new(catalog: ModelCatalog) -> Self {
        let suggestions = catalog.suggestions("");
        Self {
            catalog,
            phase: ModelPickerPhase::Model,
            query: String::new(),
            suggestions,
            selected: 0,
            scroll_offset: 0,
            pointer: ModalPointerState::default(),
        }
    }

    fn title(&self) -> &'static str {
        match self.phase {
            ModelPickerPhase::Model => "Pick model",
            ModelPickerPhase::Effort { .. } => "Pick reasoning effort",
        }
    }

    fn refresh(&mut self) {
        let args = match &self.phase {
            ModelPickerPhase::Model => self.query.clone(),
            ModelPickerPhase::Effort { model_args } => {
                format!("{model_args}{}", self.query)
            }
        };
        self.suggestions = self.catalog.suggestions(&args);
        self.selected = 0;
        self.scroll_offset = 0;
    }

    fn insert_query(&mut self, character: char) {
        self.query.push(character);
        self.refresh();
    }

    fn paste_query(&mut self, text: &str) {
        self.query
            .extend(text.chars().filter(|character| !character.is_control()));
        self.refresh();
    }

    fn backspace_query(&mut self) -> bool {
        if self.query.pop().is_some() {
            self.refresh();
            true
        } else {
            self.back_to_models()
        }
    }

    fn back_to_models(&mut self) -> bool {
        if matches!(self.phase, ModelPickerPhase::Model) {
            return false;
        }
        self.phase = ModelPickerPhase::Model;
        self.query.clear();
        self.refresh();
        true
    }

    fn move_selection(&mut self, delta: isize) {
        if self.suggestions.is_empty() || delta == 0 {
            return;
        }
        self.selected =
            (self.selected as isize + delta).rem_euclid(self.suggestions.len() as isize) as usize;
    }

    fn select(&mut self, index: usize) {
        if index < self.suggestions.len() {
            self.selected = index;
        }
    }

    fn activate(&mut self) -> ModelPickerInput {
        let Some(suggestion) = self.suggestions.get(self.selected).cloned() else {
            return ModelPickerInput::None;
        };
        let Some(args) = suggestion.insert_text.strip_prefix(MODEL_PREFIX) else {
            return ModelPickerInput::None;
        };
        if matches!(self.phase, ModelPickerPhase::Model)
            && suggestion.insert_text.ends_with(char::is_whitespace)
        {
            self.phase = ModelPickerPhase::Effort {
                model_args: args.to_string(),
            };
            self.query.clear();
            self.refresh();
            return ModelPickerInput::Redraw;
        }
        ModelPickerInput::Select(args.trim().to_string())
    }

    fn ensure_selection_visible(&mut self, height: usize) {
        if height == 0 {
            self.scroll_offset = self.selected;
            return;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset.saturating_add(height) {
            self.scroll_offset = self.selected.saturating_add(1).saturating_sub(height);
        }
        self.scroll_offset = self
            .scroll_offset
            .min(self.suggestions.len().saturating_sub(height));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModelPickerInput {
    None,
    Redraw,
    Select(String),
    Cancel,
}

pub(crate) fn handle_key(state: &mut ModelPickerState, key: KeyEvent) -> ModelPickerInput {
    if key.kind == KeyEventKind::Release {
        return ModelPickerInput::None;
    }
    match (key.code, key.modifiers) {
        (KeyCode::Esc, _) if state.back_to_models() => ModelPickerInput::Redraw,
        (KeyCode::Esc, _) => ModelPickerInput::Cancel,
        (KeyCode::Up, _) => {
            state.move_selection(-1);
            ModelPickerInput::Redraw
        }
        (KeyCode::Down, _) => {
            state.move_selection(1);
            ModelPickerInput::Redraw
        }
        (KeyCode::PageUp, _) => {
            state.move_selection(-10);
            ModelPickerInput::Redraw
        }
        (KeyCode::PageDown, _) => {
            state.move_selection(10);
            ModelPickerInput::Redraw
        }
        (KeyCode::Home, _) => {
            state.select(0);
            ModelPickerInput::Redraw
        }
        (KeyCode::End, _) => {
            state.select(state.suggestions.len().saturating_sub(1));
            ModelPickerInput::Redraw
        }
        (KeyCode::Enter, KeyModifiers::NONE) => state.activate(),
        (KeyCode::Backspace, _) => {
            if state.backspace_query() {
                ModelPickerInput::Redraw
            } else {
                ModelPickerInput::None
            }
        }
        (KeyCode::Char(character), modifiers)
            if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT =>
        {
            state.insert_query(character);
            ModelPickerInput::Redraw
        }
        _ => ModelPickerInput::None,
    }
}

pub(crate) fn handle_paste(state: &mut ModelPickerState, text: &str) -> ModelPickerInput {
    state.paste_query(text);
    ModelPickerInput::Redraw
}

pub(crate) fn handle_mouse(state: &mut ModelPickerState, mouse: MouseEvent) -> ModelPickerInput {
    match state.pointer.handle_mouse(mouse) {
        ModalPointerAction::Ignored => ModelPickerInput::None,
        ModalPointerAction::Close => ModelPickerInput::Cancel,
        ModalPointerAction::Redraw | ModalPointerAction::Hover(None) => ModelPickerInput::Redraw,
        ModalPointerAction::Hover(Some(index)) => {
            state.select(index);
            ModelPickerInput::Redraw
        }
        ModalPointerAction::Activate(index) => {
            state.select(index);
            state.activate()
        }
        ModalPointerAction::Scroll(delta) => {
            state.move_selection(delta);
            ModelPickerInput::Redraw
        }
    }
}

pub(crate) fn render_picker(
    state: &mut ModelPickerState,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
) {
    let Some(frame) = render_modal_frame_with_sizing(
        area,
        buffer,
        theme,
        state.title(),
        "↑/↓ navigate · Enter select · Esc back",
        ModalSizing::picker(),
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
    if content.is_empty() {
        state
            .pointer
            .observe_frame(frame.popup, frame.close_button, Vec::new());
        return;
    }
    buffer.set_stringn(
        content.x,
        content.y,
        format!("Search: {}", state.query),
        usize::from(content.width),
        Style::default()
            .fg(if state.query.is_empty() {
                theme.gray
            } else {
                theme.text_primary
            })
            .bg(theme.bg_base),
    );
    let list = Rect::new(
        content.x,
        content.y.saturating_add(2),
        content.width,
        content.height.saturating_sub(2),
    );
    state.ensure_selection_visible(usize::from(list.height));
    if state.suggestions.is_empty() {
        buffer.set_stringn(
            list.x,
            list.y,
            "No matching options",
            usize::from(list.width),
            Style::default().fg(theme.gray).bg(theme.bg_base),
        );
        state
            .pointer
            .observe_frame(frame.popup, frame.close_button, Vec::new());
        return;
    }
    let mut rows = Vec::new();
    for (index, suggestion) in state
        .suggestions
        .iter()
        .enumerate()
        .skip(state.scroll_offset)
        .take(usize::from(list.height))
    {
        let y =
            list.y + u16::try_from(index.saturating_sub(state.scroll_offset)).unwrap_or(u16::MAX);
        let row = Rect::new(list.x, y, list.width, 1);
        let selected = state.selected == index || state.pointer.hovered_row() == Some(index);
        let style = modal_choice_style(theme, selected);
        buffer.set_style(row, style);
        let marker = if selected { "❯ " } else { "  " };
        let label = format!("{marker}{}", suggestion.display);
        let label_width = u16::try_from(Line::from(label.as_str()).width()).unwrap_or(u16::MAX);
        let description_width =
            u16::try_from(Line::from(suggestion.description.as_str()).width()).unwrap_or(u16::MAX);
        let show_description = label_width
            .saturating_add(description_width)
            .saturating_add(2)
            < row.width;
        let description_x = if show_description {
            row.right()
                .saturating_sub(description_width)
                .saturating_sub(1)
        } else {
            row.right()
        };
        buffer.set_stringn(
            row.x,
            row.y,
            label,
            usize::from(description_x.saturating_sub(row.x).saturating_sub(1)),
            style,
        );
        if show_description {
            buffer.set_stringn(
                description_x,
                row.y,
                &suggestion.description,
                usize::from(row.right().saturating_sub(description_x)),
                Style::default()
                    .fg(if selected {
                        theme.text_primary
                    } else {
                        theme.gray
                    })
                    .bg(if selected {
                        theme.panel_selected
                    } else {
                        theme.bg_base
                    }),
            );
        }
        rows.push(ModalRowHit {
            id: index,
            area: row,
        });
    }
    state
        .pointer
        .observe_frame(frame.popup, frame.close_button, rows);
}
