use std::collections::BTreeMap;
use std::collections::BTreeSet;

use codex_app_server_protocol::ModelCapabilities;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use serde_json::Map;
use serde_json::Value;

use crate::composer::ComposerState;

use super::ModelsManagerInput;
use super::config::ConfigWriteTarget;
use super::config::ModelsConfigWrite;
use super::config::capability_write;

mod render;
mod values;

pub(super) use render::render;
use values::set_optional_number;
use values::text_value;

const FIELDS: [CapabilityField; 5] = [
    CapabilityField::ModelId,
    CapabilityField::ContextWindow,
    CapabilityField::MaxOutputTokens,
    CapabilityField::SupportsVision,
    CapabilityField::Save,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum CapabilityField {
    ModelId,
    ContextWindow,
    MaxOutputTokens,
    SupportsVision,
    Save,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CapabilityFormState {
    pub(super) provider_id: String,
    pub(super) provider_name: String,
    pub(super) editing_id: Option<String>,
    pub(super) model_id: String,
    pub(super) raw: Map<String, Value>,
    pub(super) effective: ModelCapabilities,
    pub(super) selected: usize,
    pub(super) editor: ComposerState,
    pub(super) error: Option<String>,
    draft: BTreeMap<CapabilityField, String>,
    dirty: bool,
}

impl CapabilityFormState {
    pub(super) fn provider_id(&self) -> &str {
        &self.provider_id
    }

    pub(super) fn add(provider_id: String, provider_name: String) -> Self {
        Self::new(
            provider_id,
            provider_name,
            None,
            Map::new(),
            ModelCapabilities::default(),
        )
    }

    pub(super) fn edit(
        provider_id: String,
        provider_name: String,
        model_id: String,
        raw: Map<String, Value>,
        effective: ModelCapabilities,
    ) -> Self {
        Self::new(provider_id, provider_name, Some(model_id), raw, effective)
    }

    fn new(
        provider_id: String,
        provider_name: String,
        editing_id: Option<String>,
        raw: Map<String, Value>,
        effective: ModelCapabilities,
    ) -> Self {
        let model_id = editing_id.clone().unwrap_or_default();
        let draft = text_fields()
            .into_iter()
            .map(|field| (field, text_value(&raw, config_key(field))))
            .collect();
        let mut state = Self {
            provider_id,
            provider_name,
            editing_id,
            model_id,
            raw,
            effective,
            selected: 0,
            editor: ComposerState::default(),
            error: None,
            draft,
            dirty: false,
        };
        state.load_editor();
        state
    }

    pub(super) fn fields(&self) -> &'static [CapabilityField] {
        &FIELDS
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        self.save_editor();
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.fields().len().saturating_sub(1));
        self.error = None;
        self.load_editor();
    }

    fn cycle_selection(&mut self, delta: isize) {
        self.save_editor();
        self.selected =
            (self.selected as isize + delta).rem_euclid(self.fields().len() as isize) as usize;
        self.error = None;
        self.load_editor();
    }

    pub(super) fn select(&mut self, selected: usize) {
        if selected >= self.fields().len() || selected == self.selected {
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
            (KeyCode::Left, _) if self.field().is_choice() => {
                self.cycle_choice(-1);
                ModelsManagerInput::Redraw
            }
            (KeyCode::Right, _) if self.field().is_choice() => {
                self.cycle_choice(1);
                ModelsManagerInput::Redraw
            }
            (KeyCode::Enter, KeyModifiers::NONE) => self.activate(target, existing_ids),
            _ if self.field().is_text() => {
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
        if !self.field().is_text()
            || (self.field() == CapabilityField::ModelId && self.editing_id.is_some())
        {
            return false;
        }
        let previous = self.editor.text().to_string();
        self.editor.insert_text(&text.replace(['\r', '\n'], " "));
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
            CapabilityField::Save => match self.build_write(target, existing_ids) {
                Ok(write) => ModelsManagerInput::WriteConfig(write),
                Err(error) => {
                    self.error = Some(error);
                    ModelsManagerInput::Redraw
                }
            },
            field if field.is_choice() => {
                self.cycle_choice(1);
                ModelsManagerInput::Redraw
            }
            CapabilityField::ModelId if self.editing_id.is_some() => {
                self.move_selection(1);
                ModelsManagerInput::Redraw
            }
            CapabilityField::ModelId
            | CapabilityField::ContextWindow
            | CapabilityField::MaxOutputTokens => {
                self.move_selection(1);
                ModelsManagerInput::Redraw
            }
            CapabilityField::SupportsVision => ModelsManagerInput::None,
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
        if !was_selected || self.field().is_text() {
            ModelsManagerInput::Redraw
        } else {
            self.activate(target, existing_ids)
        }
    }

    pub(super) fn draft(&self, field: CapabilityField) -> &str {
        self.draft.get(&field).map(String::as_str).unwrap_or("")
    }

    pub(super) fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn build_write(
        &mut self,
        target: Option<ConfigWriteTarget>,
        existing_ids: &BTreeSet<String>,
    ) -> Result<ModelsConfigWrite, String> {
        self.save_editor();
        let model_id = self.model_id.trim();
        if model_id.is_empty() {
            return Err("Model ID is required".to_string());
        }
        if self.editing_id.is_none() && existing_ids.contains(model_id) {
            return Err(format!("Model {model_id} already exists for this provider"));
        }
        let target = target.ok_or_else(|| {
            "The writable user config layer is unavailable; reopen Settings and try again"
                .to_string()
        })?;
        let mut raw = self.raw.clone();
        for field in text_fields() {
            let key = config_key(field);
            set_optional_number(&mut raw, key, self.draft(field))?;
        }
        Ok(capability_write(
            target,
            self.provider_id.clone(),
            model_id.to_string(),
            raw,
        ))
    }

    fn field(&self) -> CapabilityField {
        self.fields()[self.selected]
    }

    fn cycle_choice(&mut self, delta: isize) {
        let field = self.field();
        let key = config_key(field);
        let values = [None, Some(true), Some(false)];
        let current = self.raw.get(key).and_then(Value::as_bool);
        let index = values
            .iter()
            .position(|value| *value == current)
            .unwrap_or_default();
        let next = (index as isize + delta).rem_euclid(values.len() as isize) as usize;
        if let Some(value) = values[next] {
            self.raw.insert(key.to_string(), Value::Bool(value));
        } else {
            self.raw.remove(key);
        }
        self.dirty = true;
        self.error = None;
    }

    fn save_editor(&mut self) {
        let field = self.field();
        if field == CapabilityField::ModelId && self.editing_id.is_none() {
            self.model_id = self.editor.text().to_string();
        } else if field.is_text() {
            self.draft.insert(field, self.editor.text().to_string());
        }
    }

    fn load_editor(&mut self) {
        let value = if self.field() == CapabilityField::ModelId {
            self.model_id.clone()
        } else {
            self.draft(self.field()).to_string()
        };
        self.editor.replace(value);
    }
}

impl CapabilityField {
    pub(super) fn is_text(self) -> bool {
        matches!(
            self,
            Self::ModelId | Self::ContextWindow | Self::MaxOutputTokens
        )
    }

    fn is_choice(self) -> bool {
        self == Self::SupportsVision
    }
}

pub(super) fn config_key(field: CapabilityField) -> &'static str {
    match field {
        CapabilityField::ContextWindow => "context_window",
        CapabilityField::MaxOutputTokens => "max_output_tokens",
        CapabilityField::SupportsVision => "supports_vision",
        CapabilityField::ModelId | CapabilityField::Save => "",
    }
}

fn text_fields() -> [CapabilityField; 2] {
    [
        CapabilityField::ContextWindow,
        CapabilityField::MaxOutputTokens,
    ]
}
