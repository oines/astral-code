//! Provider-grouped model management state.
//!
//! The app-server remains authoritative for configuration and discovery. This
//! module only owns modal navigation, expansion, and presentation state.

mod browser;
mod capability;
mod capability_form;
mod config;
mod input;
mod provider;
mod provider_form;
mod render;

#[cfg(test)]
#[path = "models_manager_tests.rs"]
mod tests;

use std::collections::BTreeMap;

use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::Model;
use codex_app_server_protocol::ModelCapabilities;
use codex_app_server_protocol::ModelCapabilitySource;
use ratatui::layout::Rect;
use serde_json::Value;

use crate::composer::ComposerState;
use crate::modal::ModalPointerState;

pub(crate) use config::ModelsConfigWrite;
pub(crate) use input::ModelsManagerInput;
pub(crate) use input::handle_key;
pub(crate) use input::handle_mouse;
pub(crate) use input::handle_paste;
pub(crate) use render::render;

use self::browser::BrowserFocus;
use self::browser::BrowserScroll;
use self::browser::SEARCH_ROW_ID;
use self::capability_form::CapabilityFormState;
use self::config::ConfigWriteTarget;
use self::provider_form::ProviderFormState;

const DETAIL_ROW_COUNT: usize = 15;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderModelsRequest {
    pub(crate) generation: u64,
    pub(crate) provider_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ModelsManagerState {
    generation: u64,
    providers: Vec<ProviderState>,
    default_provider: Option<String>,
    current_provider: String,
    current_model: String,
    query: ComposerState,
    browser_focus: BrowserFocus,
    browser_scroll: BrowserScroll,
    selected: usize,
    scroll_offset: usize,
    detail: Option<Model>,
    detail_scroll_offset: usize,
    capability_form: Option<CapabilityFormState>,
    provider_form: Option<ProviderFormState>,
    manual_capabilities: BTreeMap<String, serde_json::Map<String, Value>>,
    write_target: Option<ConfigWriteTarget>,
    pending_request: Option<ProviderModelsRequest>,
    pointer: ModalPointerState,
    provider_toggle_hits: Vec<Option<Rect>>,
}

#[derive(Debug, Clone, PartialEq)]
struct ProviderState {
    id: String,
    name: String,
    base_url: Option<String>,
    wire_api: Option<String>,
    raw: serde_json::Map<String, Value>,
    source_label: String,
    editable: bool,
    expanded: bool,
    load: ProviderLoad,
    models: Vec<Model>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProviderLoad {
    NotLoaded,
    Loading,
    Loaded,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum BrowserRow {
    AddProvider,
    AddModel {
        provider_index: usize,
    },
    Provider {
        provider_index: usize,
    },
    EditProvider {
        provider_index: usize,
    },
    DeleteProvider {
        provider_index: usize,
    },
    Status {
        provider_index: usize,
    },
    Model {
        provider_index: usize,
        model_index: usize,
    },
}

impl ModelsManagerState {
    pub(crate) fn new(
        generation: u64,
        response: ConfigReadResponse,
        models: Vec<Model>,
        current_provider: String,
        current_model: String,
    ) -> Self {
        let default_provider = response.config.model_provider.clone();
        let write_target = config::write_target(&response);
        let manual_capabilities = config::configured_capabilities(&response);
        let provider_sources = config::configured_provider_sources(&response);
        let configured = config::configured_providers(&response);
        let mut providers = configured
            .into_iter()
            .map(|(id, provider)| {
                let source = provider_sources.get(&id);
                let name = provider
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|name| !name.is_empty())
                    .unwrap_or(&id)
                    .to_string();
                (
                    id.clone(),
                    ProviderState {
                        name,
                        base_url: provider
                            .get("base_url")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        wire_api: provider
                            .get("wire_api")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        source_label: source
                            .map(|source| source.label.clone())
                            .unwrap_or_else(|| "Built in".to_string()),
                        editable: source.is_some_and(|source| source.user_writable)
                            && !is_reserved_provider(&id),
                        raw: provider,
                        id,
                        expanded: false,
                        load: ProviderLoad::NotLoaded,
                        models: Vec::new(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();

        for model in models {
            let provider = providers
                .entry(model.model_provider.clone())
                .or_insert_with(|| ProviderState {
                    id: model.model_provider.clone(),
                    name: model.model_provider_name.clone(),
                    base_url: None,
                    wire_api: None,
                    raw: serde_json::Map::new(),
                    source_label: "Catalog".to_string(),
                    editable: false,
                    expanded: false,
                    load: ProviderLoad::NotLoaded,
                    models: Vec::new(),
                });
            provider.models.push(model);
        }
        providers
            .entry(current_provider.clone())
            .or_insert_with(|| ProviderState {
                id: current_provider.clone(),
                name: current_provider.clone(),
                base_url: None,
                wire_api: None,
                raw: serde_json::Map::new(),
                source_label: "Active session".to_string(),
                editable: false,
                expanded: false,
                load: ProviderLoad::NotLoaded,
                models: Vec::new(),
            });

        let mut providers = providers.into_values().collect::<Vec<_>>();
        providers.sort_by(|left, right| {
            (right.id == current_provider)
                .cmp(&(left.id == current_provider))
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.id.cmp(&right.id))
        });
        for provider in &mut providers {
            provider.models.sort_by(model_order);
        }
        let selected = usize::from(!providers.is_empty());

        Self {
            generation,
            providers,
            default_provider,
            current_provider,
            current_model,
            query: ComposerState::default(),
            browser_focus: BrowserFocus::List,
            browser_scroll: BrowserScroll::FollowSelection,
            selected,
            scroll_offset: 0,
            detail: None,
            detail_scroll_offset: 0,
            capability_form: None,
            provider_form: None,
            manual_capabilities,
            write_target,
            pending_request: None,
            pointer: ModalPointerState::default(),
            provider_toggle_hits: Vec::new(),
        }
    }

    pub(crate) fn take_request(&mut self) -> Option<ProviderModelsRequest> {
        self.pending_request.take()
    }

    pub(crate) fn apply_models(
        &mut self,
        provider_id: &str,
        result: Result<Vec<Model>, String>,
    ) -> bool {
        let Some(provider) = self
            .providers
            .iter_mut()
            .find(|provider| provider.id == provider_id)
        else {
            return false;
        };
        match result {
            Ok(mut models) => {
                models.sort_by(model_order);
                provider.models = models;
                provider.load = ProviderLoad::Loaded;
            }
            Err(error) => provider.load = ProviderLoad::Failed(error),
        }
        self.clamp_selection();
        true
    }

    pub(super) fn rows(&self) -> Vec<BrowserRow> {
        let query = self.query.text().trim().to_lowercase();
        let mut rows = Vec::new();
        if query.is_empty() {
            rows.push(BrowserRow::AddProvider);
        }
        for (provider_index, provider) in self.providers.iter().enumerate() {
            let provider_matches = query.is_empty()
                || contains(&provider.name, &query)
                || contains(&provider.id, &query)
                || provider
                    .base_url
                    .as_deref()
                    .is_some_and(|url| contains(url, &query));
            let matching_models = provider
                .models
                .iter()
                .enumerate()
                .filter(|(_, model)| {
                    query.is_empty()
                        || contains(&model.display_name, &query)
                        || contains(&model.model, &query)
                })
                .map(|(model_index, _)| model_index)
                .collect::<Vec<_>>();
            if !provider_matches && matching_models.is_empty() {
                continue;
            }
            rows.push(BrowserRow::Provider { provider_index });
            if provider.expanded || !query.is_empty() {
                if provider.editable && query.is_empty() {
                    rows.push(BrowserRow::EditProvider { provider_index });
                    rows.push(BrowserRow::AddModel { provider_index });
                    rows.push(BrowserRow::DeleteProvider { provider_index });
                }
                if matches!(
                    provider.load,
                    ProviderLoad::Loading | ProviderLoad::Failed(_)
                ) || provider.models.is_empty()
                {
                    rows.push(BrowserRow::Status { provider_index });
                }
                rows.extend(
                    matching_models
                        .into_iter()
                        .map(|model_index| BrowserRow::Model {
                            provider_index,
                            model_index,
                        }),
                );
            }
        }
        rows
    }

    pub(super) fn activate(&mut self, row_index: usize) -> ModelsManagerInput {
        let Some(row) = self.rows().get(row_index).cloned() else {
            return ModelsManagerInput::None;
        };
        match row {
            BrowserRow::AddProvider => {
                self.provider_form = Some(ProviderFormState::add());
                self.pointer.clear_hover();
                ModelsManagerInput::Redraw
            }
            BrowserRow::AddModel { provider_index } => {
                let provider = &self.providers[provider_index];
                self.capability_form = Some(CapabilityFormState::add(
                    provider.id.clone(),
                    provider.name.clone(),
                ));
                self.pointer.clear_hover();
                ModelsManagerInput::Redraw
            }
            BrowserRow::Provider { provider_index } => {
                let expanded = !self.providers[provider_index].expanded;
                self.set_provider_expanded(provider_index, expanded);
                ModelsManagerInput::Redraw
            }
            BrowserRow::EditProvider { provider_index } => {
                let provider = &self.providers[provider_index];
                self.provider_form = Some(ProviderFormState::edit(
                    provider.id.clone(),
                    provider.raw.clone(),
                ));
                self.pointer.clear_hover();
                ModelsManagerInput::Redraw
            }
            BrowserRow::DeleteProvider { provider_index } => {
                let provider = &self.providers[provider_index];
                let Some(target) = self.write_target.clone() else {
                    return ModelsManagerInput::Notice(
                        "The writable user config layer is unavailable; reopen Settings and try again"
                            .to_string(),
                    );
                };
                let restores_builtin = is_built_in_provider(&provider.id);
                let capability_keys = if restores_builtin {
                    Vec::new()
                } else {
                    let prefix = format!("{}/", provider.id);
                    self.manual_capabilities
                        .keys()
                        .filter(|key| key.starts_with(&prefix))
                        .cloned()
                        .collect()
                };
                let write = config::provider_delete(
                    target,
                    provider.id.clone(),
                    capability_keys,
                    !restores_builtin
                        && self.default_provider.as_deref() == Some(provider.id.as_str()),
                );
                ModelsManagerInput::ConfirmConfig {
                    title: if restores_builtin {
                        format!("Restore bundled {}?", provider.name)
                    } else {
                        format!("Remove {}?", provider.name)
                    },
                    message: if restores_builtin {
                        "This removes your provider override and restores Astral's bundled provider settings. Manual model capability overrides are kept."
                            .to_string()
                    } else {
                        "This removes the provider and its manual model capability overrides from your user config. The active session is not interrupted."
                            .to_string()
                    },
                    confirm_label: if restores_builtin {
                        "Restore bundled provider".to_string()
                    } else {
                        "Remove provider".to_string()
                    },
                    write,
                }
            }
            BrowserRow::Status { provider_index } => {
                let provider = &mut self.providers[provider_index];
                if !matches!(provider.load, ProviderLoad::Loading) {
                    provider.load = ProviderLoad::Loading;
                    self.pending_request = Some(ProviderModelsRequest {
                        generation: self.generation,
                        provider_id: provider.id.clone(),
                    });
                }
                ModelsManagerInput::Redraw
            }
            BrowserRow::Model {
                provider_index,
                model_index,
            } => {
                self.detail = self.providers[provider_index]
                    .models
                    .get(model_index)
                    .cloned();
                self.detail_scroll_offset = 0;
                self.pointer.clear_hover();
                ModelsManagerInput::Redraw
            }
        }
    }
}

fn is_reserved_provider(id: &str) -> bool {
    matches!(id, "astral" | "ollama" | "lmstudio")
}

fn is_built_in_provider(id: &str) -> bool {
    matches!(
        id,
        "astral" | "anthropic" | "amazon-bedrock" | "ollama" | "lmstudio"
    )
}

fn model_order(left: &Model, right: &Model) -> std::cmp::Ordering {
    right
        .is_default
        .cmp(&left.is_default)
        .then_with(|| left.display_name.cmp(&right.display_name))
        .then_with(|| left.model.cmp(&right.model))
}

fn contains(value: &str, query: &str) -> bool {
    value.to_lowercase().contains(query)
}

fn capability_sources(capabilities: &ModelCapabilities) -> String {
    capabilities
        .sources
        .iter()
        .map(|source| match source {
            ModelCapabilitySource::Provider => "provider",
            ModelCapabilitySource::LiteLlm => "LiteLLM",
            ModelCapabilitySource::Manual => "manual",
            ModelCapabilitySource::Fallback => "fallback",
        })
        .collect::<Vec<_>>()
        .join(" + ")
}
