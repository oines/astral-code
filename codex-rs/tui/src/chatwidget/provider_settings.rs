//! Custom-provider settings built from the TUI's existing popup primitives.

use super::*;
use codex_app_server_protocol::ConfigEdit;
use codex_app_server_protocol::ConfigLayerSource;
use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::MergeStrategy;
use serde_json::Map;
use serde_json::Value;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub(crate) enum ProviderSettingsAction {
    OpenModelPicker,
    OpenList,
    Edit(ProviderEditorDraft),
    EditText {
        draft: ProviderEditorDraft,
        field: ProviderTextField,
    },
    EditWire(ProviderEditorDraft),
    Save(ProviderEditorDraft),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ProviderTextField {
    Name,
    Id,
    BaseUrl,
    EnvKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderWire {
    ChatCompletions,
    AnthropicMessages,
}

impl ProviderWire {
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
}

#[derive(Debug, Clone)]
struct ProviderWriteTarget {
    file_path: String,
    expected_version: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderEditorDraft {
    target: ProviderWriteTarget,
    editing_id: Option<String>,
    existing_ids: BTreeSet<String>,
    raw: Map<String, Value>,
    name: String,
    id: String,
    base_url: String,
    env_key: String,
    wire: ProviderWire,
    error: Option<String>,
    selected_field_idx: usize,
}

impl ProviderEditorDraft {
    fn add(target: ProviderWriteTarget, existing_ids: BTreeSet<String>) -> Self {
        Self {
            target,
            editing_id: None,
            existing_ids,
            raw: Map::new(),
            name: String::new(),
            id: String::new(),
            base_url: String::new(),
            env_key: String::new(),
            wire: ProviderWire::ChatCompletions,
            error: None,
            selected_field_idx: 0,
        }
    }

    fn edit(
        target: ProviderWriteTarget,
        existing_ids: BTreeSet<String>,
        id: String,
        raw: Map<String, Value>,
        effective: &Map<String, Value>,
    ) -> Self {
        Self {
            target,
            editing_id: Some(id.clone()),
            existing_ids,
            name: string_value(effective, "name"),
            base_url: string_value(effective, "base_url"),
            env_key: string_value(effective, "env_key"),
            wire: if effective.get("wire_api").and_then(Value::as_str) == Some("anthropic_messages")
            {
                ProviderWire::AnthropicMessages
            } else {
                ProviderWire::ChatCompletions
            },
            id,
            raw,
            error: None,
            selected_field_idx: 0,
        }
    }

    pub(crate) fn with_text(mut self, field: ProviderTextField, value: String) -> Self {
        match field {
            ProviderTextField::Name => self.name = value,
            ProviderTextField::Id if self.editing_id.is_none() => self.id = value,
            ProviderTextField::BaseUrl => self.base_url = value,
            ProviderTextField::EnvKey => self.env_key = value,
            ProviderTextField::Id => {}
        }
        self.error = None;
        self.selected_field_idx = field.editor_index();
        self
    }

    fn with_wire(mut self, wire: ProviderWire) -> Self {
        self.wire = wire;
        self.error = None;
        self.selected_field_idx = 3;
        self
    }

    pub(crate) fn with_error(mut self, error: String) -> Self {
        self.error = Some(error);
        self.selected_field_idx = 5;
        self
    }

    pub(crate) fn config_write(&self) -> Result<(ConfigEdit, String, String), String> {
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
        if self.editing_id.is_none() && self.existing_ids.contains(id) {
            return Err(format!("Provider ID {id} already exists"));
        }

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

        Ok((
            ConfigEdit {
                key_path: quoted_key("model_providers", id),
                value: Value::Object(raw),
                merge_strategy: MergeStrategy::Replace,
            },
            self.target.file_path.clone(),
            self.target.expected_version.clone(),
        ))
    }

    fn text(&self, field: ProviderTextField) -> &str {
        match field {
            ProviderTextField::Name => &self.name,
            ProviderTextField::Id => &self.id,
            ProviderTextField::BaseUrl => &self.base_url,
            ProviderTextField::EnvKey => &self.env_key,
        }
    }
}

impl ChatWidget {
    pub(crate) fn open_model_settings_popup(&mut self) {
        let effort = self
            .current_reasoning_effort()
            .map(|effort| effort.to_string())
            .unwrap_or_else(|| "provider default".to_string());
        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Models & Providers".to_string()),
            subtitle: Some("Choose the current model or manage custom API providers.".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items: vec![
                action_item(
                    "Current model & thinking",
                    format!("{} ({effort})", self.current_model()),
                    ProviderSettingsAction::OpenModelPicker,
                ),
                action_item(
                    "Custom providers",
                    "Add or edit providers stored in your user config.".to_string(),
                    ProviderSettingsAction::OpenList,
                ),
            ],
            ..Default::default()
        });
    }

    pub(crate) fn open_custom_providers_popup(&mut self, response: ConfigReadResponse) {
        let providers = configured_providers(&response);
        let definitions = configured_provider_definitions(&response);
        let target = write_target(&response);
        let existing_ids = providers.keys().cloned().collect::<BTreeSet<_>>();
        let mut items = vec![SelectionItem {
            name: "Add custom provider".to_string(),
            description: Some(
                "Configure an OpenAI-compatible or Anthropic Messages endpoint.".to_string(),
            ),
            is_disabled: target.is_none(),
            disabled_reason: target
                .is_none()
                .then(|| "The writable user config layer is unavailable.".to_string()),
            actions: target
                .clone()
                .map(|target| {
                    let draft = ProviderEditorDraft::add(target, existing_ids.clone());
                    vec![provider_action(ProviderSettingsAction::Edit(draft))]
                })
                .unwrap_or_default(),
            dismiss_on_select: true,
            search_value: Some("add custom provider api endpoint".to_string()),
            ..Default::default()
        }];

        items.extend(providers.into_iter().map(|(id, raw)| {
            let definition = definitions
                .get(&id)
                .cloned()
                .unwrap_or_else(|| ProviderDefinition {
                    source: ProviderSource {
                        label: "Effective config".to_string(),
                        user_writable: false,
                    },
                    raw: raw.clone(),
                });
            let source = definition.source;
            let name = string_value(&raw, "name");
            let name = if name.is_empty() { id.clone() } else { name };
            let wire = raw
                .get("wire_api")
                .and_then(Value::as_str)
                .unwrap_or("chat_completions")
                .to_string();
            let base_url = string_value(&raw, "base_url");
            let is_current = self.config.model_provider_id == id;
            let description = format!(
                "{}{} · {wire} · {}",
                if is_current { "Current · " } else { "" },
                id,
                source.label
            );
            let editable_target = if source.user_writable {
                target.clone()
            } else {
                None
            };
            let can_edit = editable_target.is_some();
            let actions = if let Some(target) = editable_target {
                let draft = ProviderEditorDraft::edit(
                    target,
                    existing_ids.clone(),
                    id.clone(),
                    definition.raw,
                    &raw,
                );
                vec![provider_action(ProviderSettingsAction::Edit(draft))]
            } else {
                Vec::new()
            };
            SelectionItem {
                name,
                description: Some(description),
                is_current,
                is_disabled: !can_edit,
                disabled_reason: (!can_edit)
                    .then(|| "This provider is defined by a read-only config layer.".to_string()),
                actions,
                dismiss_on_select: true,
                search_value: Some(format!("{id} {base_url} {wire} {}", source.label)),
                ..Default::default()
            }
        }));

        self.bottom_pane.show_selection_view(SelectionViewParams {
            view_id: Some("settings-custom-providers"),
            title: Some("Custom providers".to_string()),
            subtitle: Some(
                "Built-in providers are managed by Astral and are not shown here.".to_string(),
            ),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            is_searchable: true,
            search_placeholder: Some("Search providers".to_string()),
            ..Default::default()
        });
    }

    pub(crate) fn open_provider_editor(&mut self, draft: ProviderEditorDraft) {
        let editing = draft.editing_id.is_some();
        let mut items = vec![text_field_item("Name", ProviderTextField::Name, &draft)];
        let mut id_item = text_field_item("Provider ID", ProviderTextField::Id, &draft);
        if editing {
            id_item.is_disabled = true;
            id_item.disabled_reason = Some("Provider IDs cannot be renamed.".to_string());
            id_item.actions.clear();
        }
        items.push(id_item);
        items.push(text_field_item(
            "Base URL",
            ProviderTextField::BaseUrl,
            &draft,
        ));
        items.push(SelectionItem {
            name: "API wire".to_string(),
            description: Some(draft.wire.label().to_string()),
            actions: vec![provider_action(ProviderSettingsAction::EditWire(
                draft.clone(),
            ))],
            dismiss_parent_on_child_accept: true,
            ..Default::default()
        });
        items.push(text_field_item(
            "API key environment variable",
            ProviderTextField::EnvKey,
            &draft,
        ));
        items.push(SelectionItem {
            name: "Save provider".to_string(),
            description: Some("Atomically write this provider to the user config.".to_string()),
            actions: vec![provider_action(ProviderSettingsAction::Save(draft.clone()))],
            dismiss_on_select: true,
            ..Default::default()
        });

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some(if editing {
                "Edit custom provider".to_string()
            } else {
                "Add custom provider".to_string()
            }),
            subtitle: Some(
                "Select a field to edit. Advanced keys already present are preserved.".to_string(),
            ),
            footer_note: draft
                .error
                .as_ref()
                .map(|error| Line::from(error.clone().red())),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            initial_selected_idx: Some(draft.selected_field_idx),
            ..Default::default()
        });
    }

    pub(crate) fn open_provider_text_prompt(
        &mut self,
        draft: ProviderEditorDraft,
        field: ProviderTextField,
    ) {
        let label = field_label(field);
        let initial_text = draft.text(field).to_string();
        let tx = self.app_event_tx.clone();
        let view = CustomPromptView::new(
            format!("Edit {label}"),
            format!("Enter {label}"),
            initial_text,
            Some("Changes are not saved until you choose Save provider.".to_string()),
            Box::new(move |value| {
                tx.send(AppEvent::ProviderSettings(ProviderSettingsAction::Edit(
                    draft.clone().with_text(field, value),
                )));
            }),
        );
        self.bottom_pane.show_view(Box::new(view));
    }

    pub(crate) fn open_provider_wire_picker(&mut self, draft: ProviderEditorDraft) {
        let items = [
            ProviderWire::ChatCompletions,
            ProviderWire::AnthropicMessages,
        ]
        .into_iter()
        .map(|wire| SelectionItem {
            name: wire.label().to_string(),
            description: Some(wire.value().to_string()),
            is_current: draft.wire == wire,
            actions: vec![provider_action(ProviderSettingsAction::Edit(
                draft.clone().with_wire(wire),
            ))],
            dismiss_on_select: true,
            ..Default::default()
        })
        .collect();
        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("API wire".to_string()),
            subtitle: Some("Choose the protocol exposed by the provider endpoint.".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }
}

impl ProviderTextField {
    fn editor_index(self) -> usize {
        match self {
            Self::Name => 0,
            Self::Id => 1,
            Self::BaseUrl => 2,
            Self::EnvKey => 4,
        }
    }
}

fn action_item(name: &str, description: String, action: ProviderSettingsAction) -> SelectionItem {
    SelectionItem {
        name: name.to_string(),
        description: Some(description),
        actions: vec![provider_action(action)],
        dismiss_on_select: true,
        ..Default::default()
    }
}

fn text_field_item(
    name: &str,
    field: ProviderTextField,
    draft: &ProviderEditorDraft,
) -> SelectionItem {
    let value = draft.text(field);
    SelectionItem {
        name: name.to_string(),
        description: Some(if value.is_empty() {
            "Not set".to_string()
        } else {
            value.to_string()
        }),
        actions: vec![provider_action(ProviderSettingsAction::EditText {
            draft: draft.clone(),
            field,
        })],
        dismiss_parent_on_child_accept: true,
        ..Default::default()
    }
}

fn provider_action(action: ProviderSettingsAction) -> SelectionAction {
    Box::new(move |tx| tx.send(AppEvent::ProviderSettings(action.clone())))
}

fn field_label(field: ProviderTextField) -> &'static str {
    match field {
        ProviderTextField::Name => "provider name",
        ProviderTextField::Id => "provider ID",
        ProviderTextField::BaseUrl => "base URL",
        ProviderTextField::EnvKey => "API key environment variable",
    }
}

fn configured_providers(response: &ConfigReadResponse) -> BTreeMap<String, Map<String, Value>> {
    response
        .config
        .additional
        .get("model_providers")
        .and_then(Value::as_object)
        .map(|providers| {
            providers
                .iter()
                .filter_map(|(id, value)| {
                    value.as_object().cloned().map(|value| (id.clone(), value))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Clone)]
struct ProviderSource {
    label: String,
    user_writable: bool,
}

#[derive(Clone)]
struct ProviderDefinition {
    source: ProviderSource,
    raw: Map<String, Value>,
}

fn configured_provider_definitions(
    response: &ConfigReadResponse,
) -> BTreeMap<String, ProviderDefinition> {
    let mut definitions = BTreeMap::new();
    let Some(layers) = response.layers.as_deref() else {
        return definitions;
    };
    for layer in layers
        .iter()
        .filter(|layer| layer.disabled_reason.is_none())
    {
        let Some(providers) = layer
            .config
            .get("model_providers")
            .and_then(Value::as_object)
        else {
            continue;
        };
        let source = provider_source(&layer.name);
        for (id, raw) in providers {
            let Some(raw) = raw.as_object() else {
                continue;
            };
            definitions
                .entry(id.clone())
                .or_insert_with(|| ProviderDefinition {
                    source: source.clone(),
                    raw: raw.clone(),
                });
        }
    }
    definitions
}

fn write_target(response: &ConfigReadResponse) -> Option<ProviderWriteTarget> {
    response.layers.as_deref()?.iter().find_map(|layer| {
        let ConfigLayerSource::User {
            file,
            profile: None,
        } = &layer.name
        else {
            return None;
        };
        Some(ProviderWriteTarget {
            file_path: file.to_string_lossy().to_string(),
            expected_version: layer.version.clone(),
        })
    })
}

fn provider_source(source: &ConfigLayerSource) -> ProviderSource {
    match source {
        ConfigLayerSource::User {
            file,
            profile: None,
        } => ProviderSource {
            label: format!("User · {}", file.display()),
            user_writable: true,
        },
        ConfigLayerSource::User {
            file,
            profile: Some(profile),
        } => ProviderSource {
            label: format!("User profile {profile} · {}", file.display()),
            user_writable: false,
        },
        ConfigLayerSource::Project { dot_codex_folder } => ProviderSource {
            label: format!(
                "Project override · {}",
                dot_codex_folder.join("config.toml").display()
            ),
            user_writable: false,
        },
        ConfigLayerSource::System { file } => ProviderSource {
            label: format!("System · {}", file.display()),
            user_writable: false,
        },
        ConfigLayerSource::Mdm { domain, key } => ProviderSource {
            label: format!("Managed · {domain}/{key}"),
            user_writable: false,
        },
        ConfigLayerSource::EnterpriseManaged { name, .. } => ProviderSource {
            label: format!("Managed · {name}"),
            user_writable: false,
        },
        ConfigLayerSource::SessionFlags => ProviderSource {
            label: "Session override".to_string(),
            user_writable: false,
        },
        ConfigLayerSource::LegacyManagedConfigTomlFromFile { file } => ProviderSource {
            label: format!("Managed · {}", file.display()),
            user_writable: false,
        },
        ConfigLayerSource::LegacyManagedConfigTomlFromMdm => ProviderSource {
            label: "Managed by device policy".to_string(),
            user_writable: false,
        },
    }
}

fn quoted_key(root: &str, key: &str) -> String {
    let escaped = key.replace('\\', "\\\\").replace('"', "\\\"");
    format!("{root}.\"{escaped}\"")
}

fn string_value(raw: &Map<String, Value>, key: &str) -> String {
    raw.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}
