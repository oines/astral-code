use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

use crate::composer::ComposerState;

use super::super::SettingsFocus;
use super::super::SettingsInput;
use super::super::SettingsStore;
use super::session_memory_config::push_template_edits;
use super::session_memory_config::template_label;
use super::session_memory_config::template_value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TemplateSource {
    BuiltIn,
    Inline,
    File,
}

impl TemplateSource {
    pub(super) const ALL: [Self; 3] = [Self::BuiltIn, Self::Inline, Self::File];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::BuiltIn => "Built-in",
            Self::Inline => "Inline",
            Self::File => "File",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MemoryField {
    SummarySource,
    SummaryValue,
    UpdateSource,
    UpdateValue,
    Save,
}

impl MemoryField {
    pub(super) const ALL: [Self; 5] = [
        Self::SummarySource,
        Self::SummaryValue,
        Self::UpdateSource,
        Self::UpdateValue,
        Self::Save,
    ];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::SummarySource => "Summary template source",
            Self::SummaryValue => "Summary template",
            Self::UpdateSource => "Update prompt source",
            Self::UpdateValue => "Update prompt",
            Self::Save => "Save template settings",
        }
    }

    const fn search_description(self) -> &'static str {
        match self {
            Self::SummarySource | Self::SummaryValue => {
                "built-in inline file session summary template"
            }
            Self::UpdateSource | Self::UpdateValue => {
                "built-in inline file session memory update prompt"
            }
            Self::Save => "save template settings atomically",
        }
    }

    const fn search_key(self) -> &'static str {
        match self {
            Self::SummarySource | Self::SummaryValue => {
                "session_memory_template experimental_session_memory_template_file"
            }
            Self::UpdateSource | Self::UpdateValue => {
                "session_memory_update_prompt experimental_session_memory_update_prompt_file"
            }
            Self::Save => "session memory templates save",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MemoryEditor {
    Text {
        field: MemoryField,
        input: Box<ComposerState>,
    },
    Picker {
        field: MemoryField,
        selected: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::settings) struct SessionMemoryPageState {
    pub(super) selected: usize,
    pub(super) scroll_offset: usize,
    pub(super) summary_source: TemplateSource,
    pub(super) summary_value: String,
    pub(super) update_source: TemplateSource,
    pub(super) update_value: String,
    pub(super) editor: Option<MemoryEditor>,
    pub(super) summary_dirty: bool,
    pub(super) update_dirty: bool,
    pub(super) error: Option<String>,
}

impl SessionMemoryPageState {
    pub(in crate::settings) fn new(store: &SettingsStore) -> Self {
        let (summary_source, summary_value) = template_value(
            store,
            "session_memory_template",
            "experimental_session_memory_template_file",
        );
        let (update_source, update_value) = template_value(
            store,
            "session_memory_update_prompt",
            "experimental_session_memory_update_prompt_file",
        );
        Self {
            selected: 0,
            scroll_offset: 0,
            summary_source,
            summary_value,
            update_source,
            update_value,
            editor: None,
            summary_dirty: false,
            update_dirty: false,
            error: None,
        }
    }

    pub(super) fn field(&self) -> MemoryField {
        MemoryField::ALL[self.selected.min(MemoryField::ALL.len() - 1)]
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(MemoryField::ALL.len() - 1);
    }

    pub(in crate::settings) fn set_selected(&mut self, index: usize) {
        self.selected = index.min(MemoryField::ALL.len() - 1);
    }

    pub(in crate::settings) fn query_match(query: &str) -> Option<usize> {
        MemoryField::ALL.iter().position(|field| {
            [
                field.label(),
                field.search_description(),
                field.search_key(),
            ]
            .into_iter()
            .any(|value| value.to_lowercase().contains(query))
        })
    }

    pub(super) fn value(&self, field: MemoryField) -> String {
        match field {
            MemoryField::SummarySource => self.summary_source.label().to_string(),
            MemoryField::SummaryValue => template_label(self.summary_source, &self.summary_value),
            MemoryField::UpdateSource => self.update_source.label().to_string(),
            MemoryField::UpdateValue => template_label(self.update_source, &self.update_value),
            MemoryField::Save => {
                if self.is_dirty() {
                    "Unsaved changes".to_string()
                } else {
                    "No changes".to_string()
                }
            }
        }
    }

    pub(super) fn description(&self, field: MemoryField) -> &'static str {
        match field {
            MemoryField::SummarySource => {
                "Choose the built-in template, inline text, or a file path"
            }
            MemoryField::SummaryValue => match self.summary_source {
                TemplateSource::BuiltIn => "Astral's built-in session summary template",
                TemplateSource::Inline => "Inline session summary template; paste is supported",
                TemplateSource::File => "File containing the session summary template",
            },
            MemoryField::UpdateSource => {
                "Choose the built-in updater, inline prompt, or a file path"
            }
            MemoryField::UpdateValue => match self.update_source {
                TemplateSource::BuiltIn => "Astral's built-in session-memory updater prompt",
                TemplateSource::Inline => "Inline updater prompt; paste is supported",
                TemplateSource::File => "File containing the updater prompt",
            },
            MemoryField::Save => {
                "Save both source choices atomically; inline and file keys stay mutually exclusive"
            }
        }
    }

    pub(super) fn handle_key(&mut self, store: &SettingsStore, key: KeyEvent) -> SettingsInput {
        self.error = None;
        if self.editor.is_some() {
            return self.handle_editor_key(key);
        }
        match (key.code, key.modifiers) {
            (KeyCode::Esc | KeyCode::Left | KeyCode::Char('h'), _) => SettingsInput::Close,
            (KeyCode::Char('s'), KeyModifiers::CONTROL) => self.save(store),
            (KeyCode::Up | KeyCode::Char('k'), _) => {
                self.move_selection(-1);
                SettingsInput::Redraw
            }
            (KeyCode::Down | KeyCode::Char('j'), _) => {
                self.move_selection(1);
                SettingsInput::Redraw
            }
            (KeyCode::PageUp, _) => {
                self.move_selection(-5);
                SettingsInput::Redraw
            }
            (KeyCode::PageDown, _) => {
                self.move_selection(5);
                SettingsInput::Redraw
            }
            (KeyCode::Home | KeyCode::Char('g'), _) => {
                self.set_selected(0);
                SettingsInput::Redraw
            }
            (KeyCode::End | KeyCode::Char('G'), _) => {
                self.set_selected(usize::MAX);
                SettingsInput::Redraw
            }
            (KeyCode::Char('d'), KeyModifiers::NONE) => self.reset_selected(),
            (KeyCode::Enter | KeyCode::Char(' '), KeyModifiers::NONE) => self.activate(store),
            _ => SettingsInput::None,
        }
    }

    pub(super) fn handle_paste(&mut self, text: &str) -> SettingsInput {
        if let Some(MemoryEditor::Text { input, .. }) = self.editor.as_mut() {
            input.insert_text(text);
            self.error = None;
            SettingsInput::Redraw
        } else {
            SettingsInput::None
        }
    }

    pub(super) fn activate(&mut self, store: &SettingsStore) -> SettingsInput {
        match self.field() {
            MemoryField::SummarySource | MemoryField::UpdateSource => {
                let source = if self.field() == MemoryField::SummarySource {
                    self.summary_source
                } else {
                    self.update_source
                };
                let selected = TemplateSource::ALL
                    .iter()
                    .position(|option| *option == source)
                    .unwrap_or_default();
                self.editor = Some(MemoryEditor::Picker {
                    field: self.field(),
                    selected,
                });
                SettingsInput::Redraw
            }
            MemoryField::SummaryValue if self.summary_source == TemplateSource::BuiltIn => {
                SettingsInput::Notice("Select Inline or File to provide a custom template".into())
            }
            MemoryField::UpdateValue if self.update_source == TemplateSource::BuiltIn => {
                SettingsInput::Notice("Select Inline or File to provide a custom prompt".into())
            }
            MemoryField::SummaryValue | MemoryField::UpdateValue => {
                let mut input = ComposerState::default();
                let value = if self.field() == MemoryField::SummaryValue {
                    &self.summary_value
                } else {
                    &self.update_value
                };
                input.replace(value.clone());
                self.editor = Some(MemoryEditor::Text {
                    field: self.field(),
                    input: Box::new(input),
                });
                SettingsInput::Redraw
            }
            MemoryField::Save => self.save(store),
        }
    }

    pub(super) fn activate_row(&mut self, store: &SettingsStore, index: usize) -> SettingsInput {
        self.error = None;
        if let Some(MemoryEditor::Picker { selected, .. }) = self.editor.as_mut() {
            let next = index.min(TemplateSource::ALL.len() - 1);
            if *selected != next {
                *selected = next;
                return SettingsInput::Redraw;
            }
            return self.commit_editor();
        }
        if matches!(self.editor, Some(MemoryEditor::Text { .. })) {
            return if index == 0 {
                self.commit_editor()
            } else {
                self.editor = None;
                SettingsInput::Redraw
            };
        }
        if self.selected != index {
            self.set_selected(index);
            SettingsInput::Redraw
        } else {
            self.activate(store)
        }
    }

    pub(super) fn cancel_editor(&mut self) -> bool {
        self.error = None;
        self.editor.take().is_some()
    }

    pub(super) fn handle_scroll(&mut self, delta: isize) {
        match self.editor.as_mut() {
            Some(MemoryEditor::Picker { selected, .. }) => {
                *selected = selected
                    .saturating_add_signed(delta)
                    .min(TemplateSource::ALL.len() - 1);
            }
            Some(MemoryEditor::Text { .. }) => {}
            None => self.move_selection(delta),
        }
    }

    fn handle_editor_key(&mut self, key: KeyEvent) -> SettingsInput {
        match self.editor.as_mut() {
            Some(MemoryEditor::Text { input, .. }) => match (key.code, key.modifiers) {
                (KeyCode::Esc, _) => {
                    self.editor = None;
                    SettingsInput::Redraw
                }
                (KeyCode::Enter, KeyModifiers::NONE) => self.commit_editor(),
                _ if input.edit_key(key) => SettingsInput::Redraw,
                _ => SettingsInput::None,
            },
            Some(MemoryEditor::Picker { selected, .. }) => match key.code {
                KeyCode::Esc => {
                    self.editor = None;
                    SettingsInput::Redraw
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    *selected = selected.saturating_sub(1);
                    SettingsInput::Redraw
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    *selected = (*selected + 1).min(TemplateSource::ALL.len() - 1);
                    SettingsInput::Redraw
                }
                KeyCode::Enter | KeyCode::Char(' ') => self.commit_editor(),
                _ => SettingsInput::None,
            },
            None => SettingsInput::None,
        }
    }

    fn commit_editor(&mut self) -> SettingsInput {
        let Some(editor) = self.editor.take() else {
            return SettingsInput::None;
        };
        match editor {
            MemoryEditor::Text { field, input } => {
                if input.text().trim().is_empty() {
                    self.error = Some("Custom template or path cannot be empty".to_string());
                    self.editor = Some(MemoryEditor::Text { field, input });
                    return SettingsInput::Redraw;
                }
                if field == MemoryField::SummaryValue {
                    self.summary_value = input.text().to_string();
                } else {
                    self.update_value = input.text().to_string();
                }
                self.mark_dirty(field);
            }
            MemoryEditor::Picker { field, selected } => {
                let source = TemplateSource::ALL[selected.min(TemplateSource::ALL.len() - 1)];
                if field == MemoryField::SummarySource {
                    self.summary_source = source;
                    self.summary_value.clear();
                } else {
                    self.update_source = source;
                    self.update_value.clear();
                }
                self.mark_dirty(field);
            }
        }
        SettingsInput::Redraw
    }

    fn save(&mut self, store: &SettingsStore) -> SettingsInput {
        if !self.is_dirty() {
            return SettingsInput::Notice("No template settings changed".to_string());
        }
        if (self.summary_dirty
            && self.summary_source != TemplateSource::BuiltIn
            && self.summary_value.trim().is_empty())
            || (self.update_dirty
                && self.update_source != TemplateSource::BuiltIn
                && self.update_value.trim().is_empty())
        {
            self.error = Some("Custom inline text or file path is required".to_string());
            return SettingsInput::Redraw;
        }
        let mut edits = Vec::new();
        if self.summary_dirty {
            push_template_edits(
                &mut edits,
                self.summary_source,
                &self.summary_value,
                "session_memory_template",
                "experimental_session_memory_template_file",
            );
        }
        if self.update_dirty {
            push_template_edits(
                &mut edits,
                self.update_source,
                &self.update_value,
                "session_memory_update_prompt",
                "experimental_session_memory_update_prompt_file",
            );
        }
        let Some(write) = store.write_edits(edits, SettingsFocus::SessionMemoryTemplates) else {
            return SettingsInput::Notice("User config is not writable".to_string());
        };
        SettingsInput::Write {
            write,
            selected_theme: None,
        }
    }

    pub(in crate::settings) fn is_dirty(&self) -> bool {
        self.summary_dirty || self.update_dirty
    }

    fn mark_dirty(&mut self, field: MemoryField) {
        match field {
            MemoryField::SummarySource | MemoryField::SummaryValue => {
                self.summary_dirty = true;
            }
            MemoryField::UpdateSource | MemoryField::UpdateValue => {
                self.update_dirty = true;
            }
            MemoryField::Save => {}
        }
    }

    fn reset_selected(&mut self) -> SettingsInput {
        match self.field() {
            MemoryField::SummarySource | MemoryField::SummaryValue => {
                self.summary_source = TemplateSource::BuiltIn;
                self.summary_value.clear();
                self.summary_dirty = true;
            }
            MemoryField::UpdateSource | MemoryField::UpdateValue => {
                self.update_source = TemplateSource::BuiltIn;
                self.update_value.clear();
                self.update_dirty = true;
            }
            MemoryField::Save => {
                return SettingsInput::Notice("Select a template setting to reset".to_string());
            }
        }
        SettingsInput::Redraw
    }
}
