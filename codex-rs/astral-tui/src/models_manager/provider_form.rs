use std::collections::BTreeSet;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use serde_json::Map;
use serde_json::Value;

use crate::composer::ComposerState;
use crate::modal::ModalPointerState;
use crate::modal::ModalRowHit;
use crate::view::AstralTheme;
use crate::view::ModalSizing;
use crate::view::modal_choice_style;
use crate::view::render_modal_close_button;
use crate::view::render_modal_frame_with_sizing;

use super::ModelsManagerInput;
use super::config::ConfigWriteTarget;
use super::config::ModelsConfigWrite;
use super::config::provider_write;

const FIELDS: [ProviderField; 6] = [
    ProviderField::Name,
    ProviderField::Id,
    ProviderField::BaseUrl,
    ProviderField::WireApi,
    ProviderField::EnvKey,
    ProviderField::Save,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderField {
    Name,
    Id,
    BaseUrl,
    WireApi,
    EnvKey,
    Save,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderWire {
    ChatCompletions,
    AnthropicMessages,
}

impl ProviderWire {
    fn from_raw(value: Option<&str>) -> Self {
        if value == Some("anthropic_messages") {
            Self::AnthropicMessages
        } else {
            Self::ChatCompletions
        }
    }

    fn value(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat_completions",
            Self::AnthropicMessages => "anthropic_messages",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::ChatCompletions => "Chat Completions",
            Self::AnthropicMessages => "Anthropic Messages",
        }
    }

    fn toggle(&mut self) {
        *self = match self {
            Self::ChatCompletions => Self::AnthropicMessages,
            Self::AnthropicMessages => Self::ChatCompletions,
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProviderFormState {
    editing_id: Option<String>,
    raw: Map<String, Value>,
    name: String,
    id: String,
    base_url: String,
    wire: ProviderWire,
    env_key: String,
    selected: usize,
    editor: ComposerState,
    error: Option<String>,
    dirty: bool,
}

impl ProviderFormState {
    pub(super) fn add() -> Self {
        let mut state = Self {
            editing_id: None,
            raw: Map::new(),
            name: String::new(),
            id: String::new(),
            base_url: String::new(),
            wire: ProviderWire::ChatCompletions,
            env_key: String::new(),
            selected: 0,
            editor: ComposerState::default(),
            error: None,
            dirty: false,
        };
        state.load_editor();
        state
    }

    pub(super) fn edit(id: String, raw: Map<String, Value>) -> Self {
        let mut state = Self {
            editing_id: Some(id.clone()),
            name: string_value(&raw, "name"),
            base_url: string_value(&raw, "base_url"),
            wire: ProviderWire::from_raw(raw.get("wire_api").and_then(Value::as_str)),
            env_key: string_value(&raw, "env_key"),
            raw,
            id,
            selected: 0,
            editor: ComposerState::default(),
            error: None,
            dirty: false,
        };
        state.load_editor();
        state
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        self.save_editor();
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(FIELDS.len().saturating_sub(1));
        self.error = None;
        self.load_editor();
    }

    fn cycle_selection(&mut self, delta: isize) {
        self.save_editor();
        self.selected = (self.selected as isize + delta).rem_euclid(FIELDS.len() as isize) as usize;
        self.error = None;
        self.load_editor();
    }

    pub(super) fn select(&mut self, selected: usize) {
        if selected >= FIELDS.len() || selected == self.selected {
            return;
        }
        self.save_editor();
        self.selected = selected;
        self.error = None;
        self.load_editor();
    }

    pub(super) fn handle_key(
        &mut self,
        key: KeyEvent,
        target: Option<ConfigWriteTarget>,
        existing_ids: &BTreeSet<String>,
    ) -> ModelsManagerInput {
        match (key.code, key.modifiers) {
            (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                match self.build_write(target, existing_ids) {
                    Ok(write) => ModelsManagerInput::WriteConfig(write),
                    Err(error) => {
                        self.error = Some(error);
                        ModelsManagerInput::Redraw
                    }
                }
            }
            (KeyCode::Up, _) => {
                self.move_selection(-1);
                ModelsManagerInput::Redraw
            }
            (KeyCode::Down, _) => {
                self.move_selection(1);
                ModelsManagerInput::Redraw
            }
            (KeyCode::BackTab, _) => {
                self.cycle_selection(-1);
                ModelsManagerInput::Redraw
            }
            (KeyCode::Tab, _) => {
                self.cycle_selection(1);
                ModelsManagerInput::Redraw
            }
            (KeyCode::Left | KeyCode::Right, _) if self.field() == ProviderField::WireApi => {
                self.wire.toggle();
                self.dirty = true;
                ModelsManagerInput::Redraw
            }
            (KeyCode::Enter, KeyModifiers::NONE) => self.activate(target, existing_ids),
            _ if self.field_is_text() => {
                let previous = self.editor.text().to_string();
                if self.editor.edit_key(key) {
                    self.dirty |= self.editor.text() != previous;
                    self.error = None;
                    ModelsManagerInput::Redraw
                } else {
                    ModelsManagerInput::None
                }
            }
            _ => ModelsManagerInput::None,
        }
    }

    pub(super) fn handle_paste(&mut self, text: &str) -> bool {
        if !self.field_is_text() || (self.field() == ProviderField::Id && self.editing_id.is_some())
        {
            return false;
        }
        let text = text.replace(['\r', '\n'], " ");
        let previous = self.editor.text().to_string();
        self.editor.insert_text(&text);
        self.dirty |= self.editor.text() != previous;
        self.error = None;
        true
    }

    pub(super) fn activate(
        &mut self,
        target: Option<ConfigWriteTarget>,
        existing_ids: &BTreeSet<String>,
    ) -> ModelsManagerInput {
        match self.field() {
            ProviderField::WireApi => {
                self.wire.toggle();
                self.dirty = true;
                ModelsManagerInput::Redraw
            }
            ProviderField::Save => match self.build_write(target, existing_ids) {
                Ok(write) => ModelsManagerInput::WriteConfig(write),
                Err(error) => {
                    self.error = Some(error);
                    ModelsManagerInput::Redraw
                }
            },
            ProviderField::Id if self.editing_id.is_some() => {
                self.move_selection(1);
                ModelsManagerInput::Redraw
            }
            ProviderField::Name
            | ProviderField::Id
            | ProviderField::BaseUrl
            | ProviderField::EnvKey => {
                self.move_selection(1);
                ModelsManagerInput::Redraw
            }
        }
    }

    pub(super) fn activate_pointer(
        &mut self,
        index: usize,
        target: Option<ConfigWriteTarget>,
        existing_ids: &BTreeSet<String>,
    ) -> ModelsManagerInput {
        let was_selected = self.selected == index;
        self.select(index);
        if !was_selected || self.field_is_text() {
            ModelsManagerInput::Redraw
        } else {
            self.activate(target, existing_ids)
        }
    }

    fn build_write(
        &mut self,
        target: Option<ConfigWriteTarget>,
        existing_ids: &BTreeSet<String>,
    ) -> Result<ModelsConfigWrite, String> {
        self.save_editor();
        let name = self.name.trim();
        let id = self.id.trim();
        let base_url = self.base_url.trim();
        if name.is_empty() {
            return Err("Provider name is required".to_string());
        }
        if id.is_empty() {
            return Err("Provider ID is required".to_string());
        }
        if base_url.is_empty() {
            return Err("Base URL is required".to_string());
        }
        if self.editing_id.is_none() && existing_ids.contains(id) {
            return Err(format!("Provider ID {id} already exists"));
        }
        let target = target.ok_or_else(|| {
            "The writable user config layer is unavailable; reopen Settings and try again"
                .to_string()
        })?;
        let mut raw = self.raw.clone();
        raw.insert("name".to_string(), Value::String(name.to_string()));
        raw.insert("base_url".to_string(), Value::String(base_url.to_string()));
        raw.insert(
            "wire_api".to_string(),
            Value::String(self.wire.value().to_string()),
        );
        let env_key = self.env_key.trim();
        if env_key.is_empty() {
            raw.remove("env_key");
        } else {
            raw.insert("env_key".to_string(), Value::String(env_key.to_string()));
        }
        Ok(provider_write(target, id.to_string(), raw))
    }

    pub(super) fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn field(&self) -> ProviderField {
        FIELDS[self.selected]
    }

    fn field_is_text(&self) -> bool {
        matches!(
            self.field(),
            ProviderField::Name
                | ProviderField::Id
                | ProviderField::BaseUrl
                | ProviderField::EnvKey
        )
    }

    fn save_editor(&mut self) {
        let value = self.editor.text().to_string();
        match self.field() {
            ProviderField::Name => self.name = value,
            ProviderField::Id if self.editing_id.is_none() => self.id = value,
            ProviderField::BaseUrl => self.base_url = value,
            ProviderField::EnvKey => self.env_key = value,
            ProviderField::Id | ProviderField::WireApi | ProviderField::Save => {}
        }
    }

    fn load_editor(&mut self) {
        let value = match self.field() {
            ProviderField::Name => &self.name,
            ProviderField::Id => &self.id,
            ProviderField::BaseUrl => &self.base_url,
            ProviderField::EnvKey => &self.env_key,
            ProviderField::WireApi | ProviderField::Save => "",
        };
        self.editor.replace(value);
    }
}

pub(super) fn render(
    form: &ProviderFormState,
    pointer: &mut ModalPointerState,
    area: Rect,
    buffer: &mut Buffer,
    theme: AstralTheme,
    notice: Option<(&str, bool)>,
) {
    let title = if form.editing_id.is_some() {
        "Edit provider"
    } else {
        "Add provider"
    };
    let Some(frame) = render_modal_frame_with_sizing(
        area,
        buffer,
        theme,
        title,
        "j/k fields · Enter next · Ctrl+S save · Esc",
        ModalSizing::settings(),
    ) else {
        return;
    };
    render_modal_close_button(buffer, frame.close_button, theme, pointer.close_hovered());
    let mut hits = Vec::new();
    let label_width = 20;
    let visible_slots = usize::from(frame.content.height.saturating_sub(2).saturating_add(1) / 2);
    let start = form
        .selected
        .saturating_add(1)
        .saturating_sub(visible_slots);
    for (index, field) in FIELDS.into_iter().enumerate() {
        if index < start || index >= start.saturating_add(visible_slots) {
            continue;
        }
        let y = frame.content.y
            + u16::try_from(index.saturating_sub(start).saturating_mul(2)).unwrap_or(u16::MAX);
        let row = Rect::new(frame.content.x, y, frame.content.width, 1);
        let selected = form.selected == index || pointer.hovered_row() == Some(index);
        let style = modal_choice_style(theme, selected);
        buffer.set_style(row, style);
        let marker = if selected { "❯ " } else { "  " };
        let label = match field {
            ProviderField::Name => "Name",
            ProviderField::Id => "Provider ID",
            ProviderField::BaseUrl => "Base URL",
            ProviderField::WireApi => "API wire",
            ProviderField::EnvKey => "API key env var",
            ProviderField::Save => "Save provider",
        };
        buffer.set_stringn(row.x, row.y, format!("{marker}{label}"), label_width, style);
        let value = display_value(form, field, selected);
        buffer.set_stringn(
            row.x.saturating_add(label_width as u16),
            row.y,
            value,
            usize::from(row.width.saturating_sub(label_width as u16)),
            style,
        );
        hits.push(ModalRowHit {
            id: index,
            area: row,
        });
    }
    let note_y = frame.content.bottom().saturating_sub(1);
    buffer.set_stringn(
        frame.content.x,
        note_y,
        "Provider flavor is inferred automatically. Hidden advanced keys are preserved.",
        usize::from(frame.content.width),
        Style::default().fg(theme.gray).bg(theme.bg_base),
    );
    if let Some(error) = form.error.as_deref() {
        buffer.set_stringn(
            frame.content.x,
            note_y,
            error,
            usize::from(frame.content.width),
            Style::default().fg(theme.accent_error).bg(theme.bg_base),
        );
    } else if let Some((message, is_error)) = notice {
        buffer.set_stringn(
            frame.content.x,
            note_y,
            message,
            usize::from(frame.content.width),
            Style::default()
                .fg(if is_error {
                    theme.accent_error
                } else {
                    theme.accent_running
                })
                .bg(theme.bg_base),
        );
    }
    pointer.observe_frame(frame.popup, frame.close_button, hits);
}

fn display_value(form: &ProviderFormState, field: ProviderField, selected: bool) -> String {
    match field {
        ProviderField::Name => editor_value(form, &form.name, selected),
        ProviderField::Id if form.editing_id.is_some() => format!("{}  (read-only)", form.id),
        ProviderField::Id => editor_value(form, &form.id, selected),
        ProviderField::BaseUrl => editor_value(form, &form.base_url, selected),
        ProviderField::WireApi => format!("{}  ←/→", form.wire.label()),
        ProviderField::EnvKey => editor_value(form, &form.env_key, selected),
        ProviderField::Save => "Write to user config".to_string(),
    }
}

fn editor_value(form: &ProviderFormState, committed: &str, selected: bool) -> String {
    if !selected {
        return if committed.is_empty() {
            "—".to_string()
        } else {
            committed.to_string()
        };
    }
    let value = form.editor.text();
    let cursor = form.editor.cursor().min(value.len());
    format!("{}▏{}", &value[..cursor], &value[cursor..])
}

fn string_value(raw: &Map<String, Value>, key: &str) -> String {
    raw.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}
