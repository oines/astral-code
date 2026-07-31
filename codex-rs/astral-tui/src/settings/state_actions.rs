use codex_app_server_protocol::ConfigEdit;
use codex_app_server_protocol::MergeStrategy;
use codex_protocol::openai_models::ReasoningEffort;
use serde_json::Value;

use crate::composer::ComposerState;
use crate::view::AstralThemeId;

use super::Category;
use super::SettingDefinition;
use super::SettingKind;
use super::SettingOption;
use super::SettingsConfirmAction;
use super::SettingsEditor;
use super::SettingsFocus;
use super::SettingsInput;
use super::SettingsPage;
use super::SettingsRow;
use super::SettingsState;
use super::Subpage;
use super::state::PickerOption;

impl SettingsState {
    pub(crate) fn activate_selected(&mut self) -> SettingsInput {
        let Some(row) = self.selected_row() else {
            return SettingsInput::None;
        };
        self.activate(row)
    }

    pub(crate) fn activate(&mut self, row: SettingsRow) -> SettingsInput {
        self.notice = None;
        self.notice_is_error = false;
        match row {
            SettingsRow::Category(category) => {
                self.enter_page(SettingsPage::Category(category));
                SettingsInput::Redraw
            }
            SettingsRow::Definition(definition) => self.activate_definition(definition),
            SettingsRow::Feature(index) => self.activate_feature(index),
        }
    }

    pub(crate) fn reset_selected(&mut self) -> SettingsInput {
        let Some(row) = self.selected_row() else {
            return SettingsInput::None;
        };
        let (key, label, focus) = match row {
            SettingsRow::Definition(definition) if !definition.key.is_empty() => (
                definition.key,
                definition.label,
                SettingsFocus::Key(definition.key.to_string()),
            ),
            SettingsRow::Feature(index) => {
                let feature = &self.store.data().features[index];
                let key = format!("features.{}", feature.name);
                return self.confirm_reset(
                    key,
                    feature
                        .display_name
                        .as_deref()
                        .unwrap_or(feature.name.as_str())
                        .to_string(),
                    SettingsFocus::Category(Category::Features),
                );
            }
            SettingsRow::Category(_) | SettingsRow::Definition(_) => {
                return SettingsInput::None;
            }
        };
        self.confirm_reset(key.to_string(), label.to_string(), focus)
    }

    pub(crate) fn cancel_editor(&mut self) -> Option<AstralThemeId> {
        let editor = self.editor.take()?;
        match editor {
            SettingsEditor::Picker { original_theme, .. } => original_theme,
            SettingsEditor::Text { .. } | SettingsEditor::Confirm { .. } => None,
        }
    }

    pub(crate) fn request_close(&mut self) -> SettingsInput {
        if !self.has_unsaved_drafts() {
            return SettingsInput::Close;
        }
        self.editor = Some(SettingsEditor::Confirm {
            title: "Discard unsaved changes?".to_string(),
            message: "Search provider or session-memory form changes have not been saved."
                .to_string(),
            confirm_label: "Discard and close".to_string(),
            action: SettingsConfirmAction::DiscardAndClose,
        });
        SettingsInput::Redraw
    }

    fn activate_definition(&mut self, definition: &'static SettingDefinition) -> SettingsInput {
        if let Some(reason) = self.row_disabled_reason(SettingsRow::Definition(definition)) {
            return SettingsInput::Notice(reason);
        }
        match definition.kind {
            SettingKind::Subpage(subpage) => {
                let query = self.query.text().trim().to_lowercase();
                let page = match subpage {
                    Subpage::Models => SettingsPage::Models,
                    Subpage::Search => SettingsPage::Search,
                    Subpage::SessionMemoryTemplates => SettingsPage::SessionMemoryTemplates,
                };
                self.enter_page(page);
                match subpage {
                    Subpage::Search => {
                        if let Some(index) = super::pages::SearchPageState::query_match(&query) {
                            self.search.set_selected(index);
                        }
                    }
                    Subpage::SessionMemoryTemplates => {
                        if let Some(index) =
                            super::pages::SessionMemoryPageState::query_match(&query)
                        {
                            self.session_memory.set_selected(index);
                        }
                    }
                    Subpage::Models => {}
                }
                SettingsInput::Redraw
            }
            SettingKind::Bool => self.toggle_bool(definition),
            SettingKind::Integer | SettingKind::Text => {
                let initial = self
                    .store
                    .user_value(definition.key)
                    .or_else(|| self.store.effective_value(definition.key))
                    .map(display_value)
                    .unwrap_or_default();
                let mut input = ComposerState::default();
                input.replace(initial);
                self.editor = Some(SettingsEditor::Text { definition, input });
                SettingsInput::Redraw
            }
            SettingKind::DefaultProvider | SettingKind::DefaultModel => {
                let options = self.default_model_options(definition.kind);
                if options.is_empty() {
                    return SettingsInput::Notice(
                        "No configured providers or models are available".to_string(),
                    );
                }
                self.open_owned_picker(Some(definition), None, options, None);
                SettingsInput::Redraw
            }
            SettingKind::Enum(_) if definition.key == "model_reasoning_effort" => {
                let options = self.reasoning_effort_options();
                self.open_owned_picker(Some(definition), None, options, None);
                SettingsInput::Redraw
            }
            SettingKind::Enum(options) => {
                self.open_picker(definition, options, None);
                SettingsInput::Redraw
            }
            SettingKind::PermissionProfile => {
                let options = self
                    .store
                    .data()
                    .permission_profiles
                    .iter()
                    .filter(|profile| self.permission_allowed(&profile.id))
                    .map(|profile| PickerOption {
                        label: profile.description.as_deref().map_or_else(
                            || profile.id.clone(),
                            |description| format!("{} — {description}", profile.id),
                        ),
                        value: Value::String(profile.id.clone()),
                    })
                    .collect::<Vec<_>>();
                if options.is_empty() {
                    return SettingsInput::Notice(
                        "No permission profiles are available under the current policy".to_string(),
                    );
                }
                self.open_owned_picker(Some(definition), None, options, None);
                SettingsInput::Redraw
            }
            SettingKind::Theme => {
                let options = AstralThemeId::ALL
                    .iter()
                    .map(|theme| PickerOption {
                        label: theme.label().to_string(),
                        value: Value::String(theme.config_name().to_string()),
                    })
                    .collect();
                self.open_owned_picker(Some(definition), None, options, Some(self.current_theme));
                SettingsInput::PreviewTheme(self.current_theme)
            }
        }
    }

    fn activate_feature(&mut self, index: usize) -> SettingsInput {
        if let Some(reason) = self.row_disabled_reason(SettingsRow::Feature(index)) {
            return SettingsInput::Notice(reason);
        }
        let feature = &self.store.data().features[index];
        let key = format!("features.{}", feature.name);
        let Some(write) = self.store.write_value(
            key,
            Value::Bool(!feature.enabled),
            SettingsFocus::Category(Category::Features),
        ) else {
            return SettingsInput::Notice("User config is not writable".to_string());
        };
        SettingsInput::Write {
            write,
            selected_theme: None,
        }
    }

    fn toggle_bool(&mut self, definition: &'static SettingDefinition) -> SettingsInput {
        let current = self
            .store
            .effective_value(definition.key)
            .and_then(Value::as_bool)
            .unwrap_or(definition.default == "On");
        let next = !current;
        let mut edits = vec![ConfigEdit {
            key_path: definition.key.to_string(),
            value: Value::Bool(next),
            merge_strategy: MergeStrategy::Replace,
        }];
        let conflict = match definition.key {
            "experimental_session_memory_compact" if next => {
                Some("experimental_anthropic_cached_fold")
            }
            "experimental_anthropic_cached_fold" if next => {
                Some("experimental_session_memory_compact")
            }
            _ => None,
        };
        if let Some(conflict_key) = conflict
            && self
                .store
                .effective_value(conflict_key)
                .and_then(Value::as_bool)
                == Some(true)
            && self.store.is_overridden_above_user(conflict_key)
        {
            return SettingsInput::Notice(
                "The conflicting compact strategy is enforced by a higher config layer; disable it there first."
                    .to_string(),
            );
        }
        if let Some(conflict_key) = conflict {
            edits.push(ConfigEdit {
                key_path: conflict_key.to_string(),
                value: Value::Bool(false),
                merge_strategy: MergeStrategy::Replace,
            });
        }
        let Some(write) = self
            .store
            .write_edits(edits, SettingsFocus::Key(definition.key.to_string()))
        else {
            return SettingsInput::Notice("User config is not writable".to_string());
        };
        if conflict.is_some_and(|key| {
            self.store.effective_value(key).and_then(Value::as_bool) == Some(true)
        }) {
            self.editor = Some(SettingsEditor::Confirm {
                title: format!("Enable {}?", definition.label),
                message: "This compact strategy is mutually exclusive with the currently enabled strategy. Saving will disable the other strategy atomically.".to_string(),
                confirm_label: "Enable and switch strategy".to_string(),
                action: SettingsConfirmAction::Write {
                    write,
                    selected_theme: None,
                },
            });
            SettingsInput::Redraw
        } else {
            SettingsInput::Write {
                write,
                selected_theme: None,
            }
        }
    }

    fn open_picker(
        &mut self,
        definition: &'static SettingDefinition,
        options: &'static [SettingOption],
        original_theme: Option<AstralThemeId>,
    ) {
        let options = options
            .iter()
            .filter(|option| self.option_allowed(definition.key, option.value))
            .map(|option| PickerOption {
                label: option.label.to_string(),
                value: Value::String(option.value.to_string()),
            })
            .collect::<Vec<_>>();
        if options.is_empty() {
            self.notice = Some("No values are allowed by the active configuration policy".into());
            return;
        }
        self.open_owned_picker(Some(definition), None, options, original_theme);
    }

    fn reasoning_effort_options(&self) -> Vec<PickerOption> {
        let provider = self
            .store
            .effective_value("model_provider")
            .and_then(Value::as_str);
        let configured_model = self.store.effective_value("model").and_then(Value::as_str);
        let model = self.store.data().models.iter().find(|model| {
            provider.is_none_or(|provider| model.model_provider == provider)
                && configured_model.map_or(model.is_default, |configured| model.model == configured)
        });
        let Some(model) = model else {
            return picker_options_from_static(super::registry::EFFORT);
        };
        if !model.supported_reasoning_efforts.is_empty() {
            return model
                .supported_reasoning_efforts
                .iter()
                .map(|option| PickerOption {
                    label: reasoning_effort_label(option.reasoning_effort.as_str()),
                    value: Value::String(option.reasoning_effort.as_str().to_string()),
                })
                .collect();
        }
        if model.capabilities.supports_reasoning == Some(true)
            || model.default_reasoning_effort != ReasoningEffort::None
        {
            picker_options_from_static(super::registry::EFFORT)
        } else {
            vec![PickerOption {
                label: "None".to_string(),
                value: Value::String(ReasoningEffort::None.as_str().to_string()),
            }]
        }
    }

    fn open_owned_picker(
        &mut self,
        definition: Option<&'static SettingDefinition>,
        feature_index: Option<usize>,
        options: Vec<PickerOption>,
        original_theme: Option<AstralThemeId>,
    ) {
        let current = definition.and_then(|definition| self.store.effective_value(definition.key));
        let selected = current
            .and_then(|current| options.iter().position(|option| option.value == *current))
            .unwrap_or_default();
        self.editor = Some(SettingsEditor::Picker {
            definition,
            feature_index,
            options,
            selected,
            original_theme,
        });
    }

    fn permission_allowed(&self, profile_id: &str) -> bool {
        self.store
            .data()
            .requirements
            .as_ref()
            .and_then(|requirements| requirements.allowed_permission_profiles.as_ref())
            .and_then(|profiles| profiles.get(profile_id))
            .copied()
            .unwrap_or(true)
    }

    fn option_allowed(&self, key: &str, value: &str) -> bool {
        let Some(requirements) = self.store.data().requirements.as_ref() else {
            return true;
        };
        match key {
            "web_search" => requirements
                .allowed_web_search_modes
                .as_deref()
                .is_none_or(|values| {
                    enum_value_allowed(
                        values
                            .iter()
                            .filter_map(|value| serde_json::to_value(value).ok()),
                        value,
                    )
                }),
            "approval_policy" => requirements
                .allowed_approval_policies
                .as_deref()
                .is_none_or(|values| {
                    enum_value_allowed(
                        values
                            .iter()
                            .filter_map(|value| serde_json::to_value(value).ok()),
                        value,
                    )
                }),
            "sandbox_mode" => requirements
                .allowed_sandbox_modes
                .as_deref()
                .is_none_or(|values| {
                    enum_value_allowed(
                        values
                            .iter()
                            .filter_map(|value| serde_json::to_value(value).ok()),
                        value,
                    )
                }),
            _ => true,
        }
    }

    fn confirm_reset(&mut self, key: String, label: String, focus: SettingsFocus) -> SettingsInput {
        if !self.store.has_user_override(&key) {
            return SettingsInput::Notice("This setting already inherits its value".to_string());
        }
        let Some(write) = self.store.reset(key, focus) else {
            return SettingsInput::Notice("User config is not writable".to_string());
        };
        self.editor = Some(SettingsEditor::Confirm {
            title: format!("Reset {label}?"),
            message: "Remove the user override and restore the inherited value.".to_string(),
            confirm_label: "Reset to inherited".to_string(),
            action: SettingsConfirmAction::Write {
                write,
                selected_theme: None,
            },
        });
        SettingsInput::Redraw
    }

    fn has_unsaved_drafts(&self) -> bool {
        self.search.is_dirty() || self.session_memory.is_dirty()
    }
}

fn enum_value_allowed(mut values: impl Iterator<Item = Value>, candidate: &str) -> bool {
    values.any(|value| value.as_str() == Some(candidate))
}

fn picker_options_from_static(options: &[SettingOption]) -> Vec<PickerOption> {
    options
        .iter()
        .map(|option| PickerOption {
            label: option.label.to_string(),
            value: Value::String(option.value.to_string()),
        })
        .collect()
}

fn reasoning_effort_label(value: &str) -> String {
    super::registry::EFFORT
        .iter()
        .find(|option| option.value == value)
        .map_or_else(|| value.to_string(), |option| option.label.to_string())
}

fn display_value(value: &Value) -> String {
    match value {
        Value::Null => "Inherited".to_string(),
        Value::Bool(enabled) => {
            if *enabled {
                "On".to_string()
            } else {
                "Off".to_string()
            }
        }
        Value::Number(number) => number.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(values) => values
            .iter()
            .map(display_value)
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(_) => "Configured".to_string(),
    }
}
