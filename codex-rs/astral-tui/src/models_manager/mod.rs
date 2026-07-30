//! Provider-grouped model management state.
//!
//! The app-server remains authoritative for configuration and discovery. This
//! module only owns modal navigation, expansion, and presentation state.

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
use serde_json::Value;

use crate::modal::ModalPointerState;

pub(crate) use config::ModelsConfigWrite;
pub(crate) use input::ModelsManagerInput;
pub(crate) use input::handle_key;
pub(crate) use input::handle_mouse;
pub(crate) use input::handle_paste;
pub(crate) use render::render;

use self::config::ConfigWriteTarget;
use self::provider_form::ProviderFormState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderModelsRequest {
    pub(crate) generation: u64,
    pub(crate) provider_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ModelsManagerState {
    generation: u64,
    providers: Vec<ProviderState>,
    current_provider: String,
    current_model: String,
    query: String,
    selected: usize,
    scroll_offset: usize,
    detail: Option<Model>,
    provider_form: Option<ProviderFormState>,
    write_target: Option<ConfigWriteTarget>,
    pending_request: Option<ProviderModelsRequest>,
    pointer: ModalPointerState,
}

#[derive(Debug, Clone, PartialEq)]
struct ProviderState {
    id: String,
    name: String,
    base_url: Option<String>,
    wire_api: Option<String>,
    raw: serde_json::Map<String, Value>,
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
    Provider {
        provider_index: usize,
    },
    EditProvider {
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
        let write_target = config::write_target(&response);
        let configured = config::configured_providers(&response);
        let mut providers = configured
            .into_iter()
            .map(|(id, provider)| {
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
                        editable: !is_reserved_provider(&id),
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

        Self {
            generation,
            providers,
            current_provider,
            current_model,
            query: String::new(),
            selected: 0,
            scroll_offset: 0,
            detail: None,
            provider_form: None,
            write_target,
            pending_request: None,
            pointer: ModalPointerState::default(),
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
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
        let query = self.query.trim().to_lowercase();
        let mut rows = Vec::new();
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
        if query.is_empty() {
            rows.push(BrowserRow::AddProvider);
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
                ModelsManagerInput::Redraw
            }
            BrowserRow::Provider { provider_index } => {
                let provider = &mut self.providers[provider_index];
                provider.expanded = !provider.expanded;
                if provider.expanded
                    && matches!(
                        provider.load,
                        ProviderLoad::NotLoaded | ProviderLoad::Failed(_)
                    )
                {
                    provider.load = ProviderLoad::Loading;
                    self.pending_request = Some(ProviderModelsRequest {
                        generation: self.generation,
                        provider_id: provider.id.clone(),
                    });
                }
                ModelsManagerInput::Redraw
            }
            BrowserRow::EditProvider { provider_index } => {
                let provider = &self.providers[provider_index];
                self.provider_form = Some(ProviderFormState::edit(
                    provider.id.clone(),
                    provider.raw.clone(),
                ));
                ModelsManagerInput::Redraw
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
                ModelsManagerInput::Redraw
            }
        }
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        let len = self.rows().len();
        if len > 0 {
            self.selected = (self.selected as isize + delta).rem_euclid(len as isize) as usize;
        }
    }

    pub(super) fn set_selected(&mut self, selected: usize) {
        if selected < self.rows().len() {
            self.selected = selected;
        }
    }

    pub(super) fn clamp_selection(&mut self) {
        self.selected = self.selected.min(self.rows().len().saturating_sub(1));
    }
}

fn is_reserved_provider(id: &str) -> bool {
    matches!(id, "astral" | "ollama" | "lmstudio")
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
