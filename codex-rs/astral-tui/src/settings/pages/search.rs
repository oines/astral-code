use std::collections::BTreeSet;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use serde_json::Value;

use crate::composer::ComposerState;

use super::super::SettingsInput;
use super::super::SettingsStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum SearchField {
    Provider,
    ApiKey,
    Save,
}

impl SearchField {
    pub(super) const ALL: [Self; 3] = [Self::Provider, Self::ApiKey, Self::Save];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Provider => "Provider",
            Self::ApiKey => "API key",
            Self::Save => "Save search settings",
        }
    }

    pub(super) const fn description(self) -> &'static str {
        match self {
            Self::Provider => "Tavily, Exa, Jina, Brave, or SerpAPI",
            Self::ApiKey => "Keep, replace, or clear the stored secret",
            Self::Save => "Write every changed field atomically to the user config",
        }
    }

    pub(super) const fn key(self) -> Option<&'static str> {
        match self {
            Self::Provider => Some("tools.web_search.provider"),
            Self::ApiKey => Some("tools.web_search.api_key"),
            Self::Save => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SecretDraft {
    Keep,
    Replace(String),
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SearchEditor {
    Text {
        field: SearchField,
        input: Box<ComposerState>,
    },
    Picker {
        field: SearchField,
        options: Vec<(String, Option<String>)>,
        selected: usize,
    },
    Secret {
        selected: usize,
    },
}

#[derive(Clone, PartialEq, Eq)]
pub(in crate::settings) struct SearchPageState {
    pub(super) selected: usize,
    pub(super) scroll_offset: usize,
    pub(super) provider: Option<String>,
    pub(super) secret: SecretDraft,
    pub(super) secret_configured: bool,
    pub(super) editor: Option<SearchEditor>,
    pub(super) changed: BTreeSet<SearchField>,
    pub(super) error: Option<String>,
}

impl std::fmt::Debug for SearchPageState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SearchPageState")
            .field("selected", &self.selected)
            .field("scroll_offset", &self.scroll_offset)
            .field("provider", &self.provider)
            .field("secret", &"<redacted>")
            .field("secret_configured", &self.secret_configured)
            .field("editor", &self.editor.as_ref().map(|_| "<redacted>"))
            .field("changed", &self.changed)
            .field("error", &self.error)
            .finish()
    }
}

impl SearchPageState {
    pub(in crate::settings) fn new(store: &SettingsStore) -> Self {
        let value = |key| {
            store
                .effective_value(key)
                .or_else(|| store.user_value(key))
                .and_then(Value::as_str)
                .map(str::to_string)
        };
        let secret_configured = store
            .effective_value("tools.web_search.api_key")
            .or_else(|| store.user_value("tools.web_search.api_key"))
            .and_then(Value::as_str)
            .is_some_and(|secret| !secret.is_empty());
        Self {
            selected: 0,
            scroll_offset: 0,
            provider: value("tools.web_search.provider"),
            secret: SecretDraft::Keep,
            secret_configured,
            editor: None,
            changed: BTreeSet::new(),
            error: None,
        }
    }

    pub(super) fn field(&self) -> SearchField {
        SearchField::ALL[self.selected.min(SearchField::ALL.len() - 1)]
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(SearchField::ALL.len() - 1);
    }

    pub(in crate::settings) fn set_selected(&mut self, index: usize) {
        self.selected = index.min(SearchField::ALL.len() - 1);
    }

    pub(in crate::settings) fn query_match(query: &str) -> Option<usize> {
        SearchField::ALL.iter().position(|field| {
            [Some(field.label()), Some(field.description()), field.key()]
                .into_iter()
                .flatten()
                .any(|value| value.to_lowercase().contains(query))
        })
    }

    pub(super) fn value(&self, field: SearchField) -> String {
        match field {
            SearchField::Provider => self.provider.as_deref().unwrap_or("Not configured").into(),
            SearchField::ApiKey => match &self.secret {
                SecretDraft::Keep if self.secret_configured => "Configured · keep".to_string(),
                SecretDraft::Keep => "Not configured".to_string(),
                SecretDraft::Replace(_) => "Replace on save".to_string(),
                SecretDraft::Clear => "Clear on save".to_string(),
            },
            SearchField::Save => {
                if self.is_dirty() {
                    "Unsaved changes".to_string()
                } else {
                    "No changes".to_string()
                }
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
        if let Some(SearchEditor::Text { input, .. }) = self.editor.as_mut() {
            input.insert_text(text);
            self.error = None;
            SettingsInput::Redraw
        } else {
            SettingsInput::None
        }
    }

    pub(super) fn activate(&mut self, store: &SettingsStore) -> SettingsInput {
        match self.field() {
            SearchField::Provider => {
                let current = self.provider.clone();
                self.open_picker(
                    SearchField::Provider,
                    &["tavily", "exa", "jina", "brave", "serpapi"],
                    current.as_deref(),
                    /*allow_none*/ true,
                )
            }
            SearchField::ApiKey => {
                self.editor = Some(SearchEditor::Secret { selected: 0 });
                SettingsInput::Redraw
            }
            SearchField::Save => self.save(store),
        }
    }

    pub(super) fn activate_row(&mut self, store: &SettingsStore, index: usize) -> SettingsInput {
        self.error = None;
        if let Some(editor) = self.editor.as_mut() {
            match editor {
                SearchEditor::Picker {
                    options, selected, ..
                } => {
                    let next = index.min(options.len().saturating_sub(1));
                    if *selected != next {
                        *selected = next;
                        return SettingsInput::Redraw;
                    }
                }
                SearchEditor::Secret { selected } => {
                    let next = index.min(2);
                    if *selected != next {
                        *selected = next;
                        return SettingsInput::Redraw;
                    }
                }
                SearchEditor::Text { .. } => {
                    return if index == 0 {
                        self.commit_editor()
                    } else {
                        self.editor = None;
                        SettingsInput::Redraw
                    };
                }
            }
            return self.commit_editor();
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
            Some(SearchEditor::Picker {
                options, selected, ..
            }) => {
                *selected = selected
                    .saturating_add_signed(delta)
                    .min(options.len().saturating_sub(1));
            }
            Some(SearchEditor::Secret { selected }) => {
                *selected = selected.saturating_add_signed(delta).min(2);
            }
            Some(SearchEditor::Text { .. }) => {}
            None => self.move_selection(delta),
        }
    }

    pub(in crate::settings) fn is_dirty(&self) -> bool {
        !self.changed.is_empty()
    }
}

pub(super) fn display_provider_value(value: &str) -> String {
    match value {
        "serpapi" => "SerpAPI".to_string(),
        _ => {
            let mut chars = value.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        }
    }
}
