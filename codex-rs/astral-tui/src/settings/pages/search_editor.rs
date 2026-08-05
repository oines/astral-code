use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

use crate::composer::ComposerState;

use super::super::SettingsInput;
use super::search::SearchEditor;
use super::search::SearchField;
use super::search::SearchPageState;
use super::search::SecretDraft;
use super::search::display_provider_value;

impl SearchPageState {
    pub(super) fn handle_editor_key(&mut self, key: KeyEvent) -> SettingsInput {
        match self.editor.as_mut() {
            Some(SearchEditor::Text { input, .. }) => match (key.code, key.modifiers) {
                (KeyCode::Esc, _) => {
                    self.editor = None;
                    SettingsInput::Redraw
                }
                (KeyCode::Enter, KeyModifiers::NONE) => self.commit_editor(),
                _ if input.edit_key(key) => SettingsInput::Redraw,
                _ => SettingsInput::None,
            },
            Some(SearchEditor::Picker {
                options, selected, ..
            }) => match (key.code, key.modifiers) {
                (KeyCode::Esc, _) => {
                    self.editor = None;
                    SettingsInput::Redraw
                }
                (KeyCode::Up | KeyCode::Char('k'), _) => {
                    *selected = selected.saturating_sub(1);
                    SettingsInput::Redraw
                }
                (KeyCode::Down | KeyCode::Char('j'), _) => {
                    *selected = (*selected + 1).min(options.len().saturating_sub(1));
                    SettingsInput::Redraw
                }
                (KeyCode::Enter | KeyCode::Char(' '), KeyModifiers::NONE) => self.commit_editor(),
                _ => SettingsInput::None,
            },
            Some(SearchEditor::Secret { selected }) => match key.code {
                KeyCode::Esc => {
                    self.editor = None;
                    SettingsInput::Redraw
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    *selected = selected.saturating_sub(1);
                    SettingsInput::Redraw
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    *selected = (*selected + 1).min(2);
                    SettingsInput::Redraw
                }
                KeyCode::Enter | KeyCode::Char(' ') => self.commit_editor(),
                _ => SettingsInput::None,
            },
            None => SettingsInput::None,
        }
    }

    pub(super) fn commit_editor(&mut self) -> SettingsInput {
        let Some(editor) = self.editor.take() else {
            return SettingsInput::None;
        };
        match editor {
            SearchEditor::Text { field, input } => {
                let value = input.text().to_string();
                if value.trim().is_empty() || value == "[redacted]" {
                    self.error =
                        Some("Enter a real API key; [redacted] is never written".to_string());
                    self.editor = Some(SearchEditor::Text { field, input });
                    return SettingsInput::Redraw;
                }
                self.secret = SecretDraft::Replace(value);
                self.changed.insert(SearchField::ApiKey);
            }
            SearchEditor::Picker {
                field,
                options,
                selected,
            } => {
                let value = options.get(selected).and_then(|(_, value)| value.clone());
                match field {
                    SearchField::Provider => self.provider = value,
                    SearchField::ApiKey | SearchField::Save => {}
                }
                self.changed.insert(field);
            }
            SearchEditor::Secret { selected } => match selected {
                0 => {
                    self.secret = SecretDraft::Keep;
                    self.changed.remove(&SearchField::ApiKey);
                }
                1 => {
                    let input = ComposerState::default();
                    self.editor = Some(SearchEditor::Text {
                        field: SearchField::ApiKey,
                        input: Box::new(input),
                    });
                    return SettingsInput::Redraw;
                }
                _ => {
                    self.secret = SecretDraft::Clear;
                    self.changed.insert(SearchField::ApiKey);
                }
            },
        }
        SettingsInput::Redraw
    }

    pub(super) fn open_picker(
        &mut self,
        field: SearchField,
        values: &[&str],
        current: Option<&str>,
        allow_none: bool,
    ) -> SettingsInput {
        let mut options = Vec::new();
        if allow_none {
            let label = match field {
                SearchField::Provider => "Not configured",
                SearchField::ApiKey | SearchField::Save => "Not set",
            };
            options.push((label.to_string(), None));
        }
        options.extend(
            values
                .iter()
                .map(|value| (display_provider_value(value), Some((*value).to_string()))),
        );
        let selected = options
            .iter()
            .position(|(_, value)| value.as_deref() == current)
            .unwrap_or_default();
        self.editor = Some(SearchEditor::Picker {
            field,
            options,
            selected,
        });
        SettingsInput::Redraw
    }

    pub(super) fn reset_selected(&mut self) -> SettingsInput {
        let field = self.field();
        match field {
            SearchField::Provider => self.provider = None,
            SearchField::ApiKey => self.secret = SecretDraft::Clear,
            SearchField::Save => {
                return SettingsInput::Notice("Select a field to reset".to_string());
            }
        }
        self.changed.insert(field);
        SettingsInput::Redraw
    }
}
